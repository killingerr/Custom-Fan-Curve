//! Daemon implementation for the fan curve application

use crate::{
    alerts::fan_control,
    channels::{ChannelAssignment, GpuReading, GpuTemp, IoBoard, TempSource},
    cpu_temp::CpuTempDetector,
    errors::{FanCurveError, Result},
    fan::{FanCurve, FanCurveConfig},
    overheat::OverheatGuard,
    DBUS_OBJECT_PATH, DBUS_SERVICE_NAME,
};
use log::{debug, error, info, warn};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use zbus::fdo::Error::Failed;
use zbus::{dbus_interface, ConnectionBuilder, SignalContext};

const SYSTEM76_POWER_SERVICE: &str = "com.system76.PowerDaemon";
const SYSTEM76_POWER_PATH: &str = "/com/system76/PowerDaemon";
const SYSTEM76_POWER_INTERFACE: &str = "com.system76.PowerDaemon";

/// Live telemetry published by the control loop for DBus clients
#[derive(Clone, Debug, Default)]
pub struct DaemonStatus {
    pub temperature: f64,
    /// Duty in ten-thousandths (0-10000)
    pub duty: u16,
    /// PWM value written to hwmon (0-255)
    pub pwm: u8,
    pub fan_speeds: Vec<(u8, u16, String)>,
    /// Overheat override: (active, hottest °C, "CPU" or "GPU")
    pub overheat: (bool, f64, String),
    /// Who controls the fans: a state id from `alerts::fan_control`, and detail
    pub fan_control: (String, String),
    /// Latest CPU temperature (°C), if readable
    pub cpu_temp: Option<f32>,
    /// Latest GPU temperature reading
    pub gpu_temp: Option<GpuReading>,
    /// Thelio Io channels: (channel, label, source followed, source when
    /// synced, duty 0-10000)
    pub channels: Vec<(u8, String, TempSource, TempSource, u16)>,
}

/// Main daemon structure
pub struct FanCurveDaemon {
    config: Arc<Mutex<FanCurveConfig>>,
    current_curve_index: Arc<Mutex<usize>>,
    status: Arc<Mutex<DaemonStatus>>,
}

impl FanCurveDaemon {
    /// Create a new daemon instance
    pub fn new() -> Result<Self> {
        let loaded = Self::load_config()?;
        // Always start on the persisted default so behavior survives reboots
        let initial_index = loaded
            .default_curve_index
            .unwrap_or(0)
            .min(loaded.curves.len().saturating_sub(1));
        let default_name = loaded
            .curves
            .get(initial_index)
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "(none)".to_string());
        info!(
            "Loaded config from {}; default curve: '{}' (index {})",
            FanCurveConfig::get_config_path().display(),
            default_name,
            initial_index
        );

        Ok(Self {
            config: Arc::new(Mutex::new(loaded)),
            current_curve_index: Arc::new(Mutex::new(initial_index)),
            status: Arc::new(Mutex::new(DaemonStatus {
                fan_control: (fan_control::STARTING.to_string(), String::new()),
                ..Default::default()
            })),
        })
    }

    /// Load configuration from file or create default
    fn load_config() -> Result<FanCurveConfig> {
        let config_path = FanCurveConfig::get_config_path();
        if config_path.exists() {
            FanCurveConfig::load_from_file(&config_path)
                .map_err(|e| FanCurveError::Config(format!("Failed to load config: {}", e)))
        } else {
            let config = FanCurveConfig::new();
            config.save_to_file(&config_path).map_err(|e| {
                FanCurveError::Config(format!("Failed to save default config: {}", e))
            })?;
            Ok(config)
        }
    }

    /// Index of the curve called `name`
    fn curve_index(&self, name: &str) -> zbus::fdo::Result<usize> {
        let config = self.config.lock().unwrap();
        config
            .curves
            .iter()
            .position(|c| c.name() == name)
            .ok_or_else(|| Failed(format!("Fan curve not found: {}", name)))
    }

    /// Persist the in-memory config, reporting failure as a DBus error
    fn save(&self) -> zbus::fdo::Result<()> {
        let path = FanCurveConfig::get_config_path();
        self.config
            .lock()
            .unwrap()
            .save_to_file(&path)
            .map_err(|e| {
                error!("Failed to save config: {}", e);
                Failed(format!("Failed to save config: {}", e))
            })?;
        info!("Configuration saved to {}", path.display());
        Ok(())
    }

    /// Run the daemon: serve the DBus interface and drive the fan control loop
    pub async fn run(self) -> Result<()> {
        info!("Starting fan curve daemon");

        let config = Arc::clone(&self.config);
        let current_curve_index = Arc::clone(&self.current_curve_index);
        let status = Arc::clone(&self.status);

        let connection = ConnectionBuilder::system()?
            .name(DBUS_SERVICE_NAME)?
            .serve_at(DBUS_OBJECT_PATH, self)?
            .build()
            .await?;

        info!("Daemon started, listening on DBus");

        run_control_loop(&connection, config, current_curve_index, status).await;
        Ok(())
    }
}

/// Error for attempts to change a built-in profile.
const BUILTIN_READ_ONLY: &str =
    "Built-in profiles are read-only; save a copy as a new profile to change it";

/// How often the daemon re-sends ClaimFanControl, in control loop ticks.
/// Re-claiming picks the claim back up after system76-power restarts (it
/// forgets claims), after another claimant releases, or once system76-power
/// starts or gains claim support.
const CLAIM_INTERVAL_TICKS: u32 = 5;

/// How often the daemon looks for a Thelio Io board while none is found, in
/// control loop ticks.
const BOARD_RESCAN_TICKS: u32 = 5;

/// Result of asking system76-power for fan control.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ClaimOutcome {
    /// system76-power paused its curve for this daemon.
    Granted,
    /// Another program holds the claim; this daemon must not write the fans.
    HeldByOther,
    /// system76-power isn't on the bus, so there is no claim and this daemon
    /// must not write the fans.
    NotRunning,
    /// system76-power predates ClaimFanControl; its curve keeps running.
    Unsupported,
    /// Any other refusal (e.g. polkit unavailable, bus errors). system76-power's
    /// curve keeps running, so this daemon must not write the fans.
    Failed(String),
}

impl ClaimOutcome {
    /// State id and detail published over DBus (see `alerts::fan_control`).
    fn state(&self) -> (&'static str, String) {
        match self {
            ClaimOutcome::Granted => (fan_control::CLAIMED, String::new()),
            ClaimOutcome::NotRunning => (fan_control::DIRECT, String::new()),
            ClaimOutcome::Unsupported => (fan_control::SHARED, String::new()),
            ClaimOutcome::HeldByOther => (fan_control::HELD_BY_OTHER, String::new()),
            ClaimOutcome::Failed(why) => (fan_control::REFUSED, why.clone()),
        }
    }

    /// Return whether this daemon should write the fans, which it does only
    /// while it holds the claim.
    fn may_write(&self) -> bool {
        matches!(self, ClaimOutcome::Granted)
    }

    fn log(&self) {
        match self {
            ClaimOutcome::Granted => info!("Claimed fan control from system76-power"),
            ClaimOutcome::HeldByOther => warn!(
                "Another program holds the fan control claim; \
                 not writing the fans until it releases"
            ),
            ClaimOutcome::NotRunning => warn!(
                "system76-power is not running; \
                 not writing the fans until it grants the claim"
            ),
            ClaimOutcome::Unsupported => warn!(
                "system76-power does not support ClaimFanControl; \
                 leaving the fans to its curve"
            ),
            ClaimOutcome::Failed(why) => warn!(
                "system76-power refused the fan control claim ({}); \
                 leaving the fans to its curve and retrying",
                why
            ),
        }
    }
}

/// Ask system76-power to pause its fan curve so this daemon can drive the
/// fans. The claim is held by `connection` and is dropped automatically by
/// system76-power if this process exits without releasing it. Claiming again
/// while already holding the claim succeeds.
async fn claim_fan_control(connection: &zbus::Connection) -> ClaimOutcome {
    match connection
        .call_method(
            Some(SYSTEM76_POWER_SERVICE),
            SYSTEM76_POWER_PATH,
            Some(SYSTEM76_POWER_INTERFACE),
            "ClaimFanControl",
            &(),
        )
        .await
    {
        Ok(_) => ClaimOutcome::Granted,
        Err(zbus::Error::MethodError(name, msg, _)) => {
            let msg = msg.unwrap_or_default();
            match name.as_str() {
                "org.freedesktop.DBus.Error.ServiceUnknown" => ClaimOutcome::NotRunning,
                "org.freedesktop.DBus.Error.UnknownMethod" => ClaimOutcome::Unsupported,
                // system76-power reports a held claim only through this message.
                _ if msg.contains("already claimed") => ClaimOutcome::HeldByOther,
                _ => ClaimOutcome::Failed(format!("{}: {}", name, msg)),
            }
        }
        Err(e) => ClaimOutcome::Failed(e.to_string()),
    }
}

/// Hand fan control back to system76-power, which returns the fans to
/// automatic control and resumes its own fan curve.
async fn release_fan_control(connection: &zbus::Connection) -> bool {
    match connection
        .call_method(
            Some(SYSTEM76_POWER_SERVICE),
            SYSTEM76_POWER_PATH,
            Some(SYSTEM76_POWER_INTERFACE),
            "ReleaseFanControl",
            &(),
        )
        .await
    {
        Ok(_) => {
            info!("Released fan control to system76-power");
            true
        }
        Err(e) => {
            warn!("Failed to release fan control to system76-power: {}", e);
            false
        }
    }
}

/// Record who controls the fans and tell DBus clients.
async fn publish_fan_control(
    connection: &zbus::Connection,
    status: &Arc<Mutex<DaemonStatus>>,
    claim: &ClaimOutcome,
) {
    let (state, detail) = claim.state();
    status.lock().unwrap().fan_control = (state.to_string(), detail.clone());
    match SignalContext::new(connection, DBUS_OBJECT_PATH) {
        Ok(ctxt) => {
            if let Err(e) = FanCurveDaemon::fan_control_changed(&ctxt, state, &detail).await {
                warn!("Failed to emit FanControlChanged signal: {}", e);
            }
        }
        Err(e) => warn!("Failed to emit FanControlChanged signal: {}", e),
    }
}

/// Record the overheat override state, and tell DBus clients when it turns
/// on or off.
async fn publish_overheat(
    connection: &zbus::Connection,
    status: &Arc<Mutex<DaemonStatus>>,
    overheat: &OverheatGuard,
    changed: bool,
) {
    let (temperature, sensor) = overheat.hottest().unwrap_or((0.0, "CPU"));
    let state = (
        overheat.is_active(),
        f64::from(temperature),
        sensor.to_string(),
    );
    status.lock().unwrap().overheat = state.clone();
    if !changed {
        return;
    }
    match SignalContext::new(connection, DBUS_OBJECT_PATH) {
        Ok(ctxt) => {
            if let Err(e) =
                FanCurveDaemon::overheat_changed(&ctxt, state.0, state.1, &state.2).await
            {
                warn!("Failed to emit OverheatChanged signal: {}", e);
            }
        }
        Err(e) => warn!("Failed to emit OverheatChanged signal: {}", e),
    }
}

/// Periodically read the CPU temperature, look up the duty on the current
/// curve, and apply it to the Thelio Io board's fans while this daemon holds
/// the fan control claim. On shutdown, it hands control back to system76-power,
/// or returns the fans to automatic control itself if the release fails.
async fn run_control_loop(
    connection: &zbus::Connection,
    config: Arc<Mutex<FanCurveConfig>>,
    current_curve_index: Arc<Mutex<usize>>,
    status: Arc<Mutex<DaemonStatus>>,
) {
    let mut cpu_temp = CpuTempDetector::new();
    if let Err(e) = cpu_temp.initialize() {
        warn!("Control loop: CPU temperature detection unavailable: {}", e);
    }

    let mut board = IoBoard::discover();
    let mut gpu_temp = GpuTemp::discover();

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();

    let mut claim = claim_fan_control(connection).await;
    claim.log();
    publish_fan_control(connection, &status, &claim).await;
    let mut overheat = OverheatGuard::new();
    // Last outcome seen, so each change is logged once.
    let mut last_outcome = claim.clone();
    let mut tick: u32 = 0;

    loop {
        tokio::select! {
            _ = sleep(Duration::from_secs(1)) => {
                tick = tick.wrapping_add(1);
                if tick % CLAIM_INTERVAL_TICKS == 0 {
                    let outcome = claim_fan_control(connection).await;
                    if outcome != last_outcome {
                        outcome.log();
                        last_outcome = outcome.clone();
                    }
                    // system76-power checks authorization before touching the
                    // claim, so a failed re-claim leaves a held claim in place.
                    let still_held = claim == ClaimOutcome::Granted
                        && matches!(outcome, ClaimOutcome::Failed(_));
                    if !still_held && outcome != claim {
                        claim = outcome;
                        publish_fan_control(connection, &status, &claim).await;
                    }
                }

                // The board's driver may load after the daemon starts at boot.
                if board.is_none() && tick % BOARD_RESCAN_TICKS == 0 {
                    board = IoBoard::discover();
                }

                // Read temperatures every tick, even when not writing the fans,
                // so clients can always show them.
                let cpu = read_cpu_temp(&cpu_temp);
                let gpu = gpu_temp.read();
                {
                    let mut st = status.lock().unwrap();
                    st.cpu_temp = cpu;
                    st.gpu_temp = Some(gpu);
                }

                if !claim.may_write() {
                    continue;
                }

                let was_overheating = overheat.is_active();

                if let Some(board) = board.as_mut() {
                    board_step(
                        &config,
                        &current_curve_index,
                        &status,
                        board,
                        cpu,
                        gpu.value(),
                        &mut overheat,
                    );
                }
                let changed = overheat.is_active() != was_overheating;
                publish_overheat(connection, &status, &overheat, changed).await;
            }
            _ = &mut ctrl_c => {
                info!("Received SIGINT, shutting down");
                break;
            }
            _ = async {
                match sigterm.as_mut() {
                    Some(s) => { s.recv().await; }
                    None => std::future::pending::<()>().await,
                }
            } => {
                info!("Received SIGTERM, shutting down");
                break;
            }
        }
    }

    let released = claim == ClaimOutcome::Granted && release_fan_control(connection).await;
    if !restore_fans_on_exit(&claim, released) {
        return;
    }

    if let Some(board) = &board {
        info!("Returning fans to automatic control");
        board.set_automatic();
    }
}

/// Whether the daemon must return the fans it drove to automatic control
/// itself when it stops.
fn restore_fans_on_exit(claim: &ClaimOutcome, released: bool) -> bool {
    // Without the claim this daemon never wrote the fans, so they aren't its
    // to touch. On release system76-power returns the fans to automatic and
    // resumes its curve.
    claim.may_write() && !released
}

/// Current CPU temperature (°C), or None if it can't be read.
fn read_cpu_temp(cpu_temp: &CpuTempDetector) -> Option<f32> {
    if !cpu_temp.is_initialized() {
        return None;
    }
    match cpu_temp.read_temperature() {
        Ok(t) => Some(t),
        Err(e) => {
            warn!("Control loop: failed to read temperature: {}", e);
            None
        }
    }
}

/// One iteration of the control loop on a Thelio Io board: every channel
/// follows its own curve and temperature source.
fn board_step(
    config: &Arc<Mutex<FanCurveConfig>>,
    current_curve_index: &Arc<Mutex<usize>>,
    status: &Arc<Mutex<DaemonStatus>>,
    board: &mut IoBoard,
    cpu: Option<f32>,
    gpu: Option<f32>,
    overheat: &mut OverheatGuard,
) {
    let full_speed = overheat.update(cpu, gpu);

    // Write every tick, even when unchanged: Thelio Io boards take over
    // the fans on their own if nothing is written for several seconds.
    let written = {
        let cfg = config.lock().unwrap();
        let index = *current_curve_index.lock().unwrap();
        board.apply(&cfg, index, cpu, gpu, full_speed)
    };

    // GetStatus reports a single duty; use the CPU fan channel's.
    let (duty, pwm) = written
        .iter()
        .find(|(channel, _, _)| *channel == 1)
        .or(written.first())
        .map(|&(_, duty, pwm)| (duty, pwm))
        .unwrap_or_default();

    let channels = {
        let cfg = config.lock().unwrap();
        let index = *current_curve_index.lock().unwrap();
        board
            .channels()
            .iter()
            .map(|c| {
                let duty = written
                    .iter()
                    .find(|(n, _, _)| *n == c.number)
                    .map_or(0, |&(_, duty, _)| duty);
                (
                    c.number,
                    c.label.clone(),
                    board.source(&cfg, index, c.number),
                    board.synced_source(&cfg, c.number),
                    duty,
                )
            })
            .collect()
    };

    let mut st = status.lock().unwrap();
    st.temperature = f64::from(cpu.or(gpu).unwrap_or_default());
    st.duty = duty;
    st.pwm = pwm;
    st.fan_speeds = board.read_speeds();
    st.channels = channels;
}

#[dbus_interface(name = "com.system76.FanCurveDaemon")]
impl FanCurveDaemon {
    /// Signal emitted when fan curve changes
    #[dbus_interface(signal)]
    async fn fan_curve_changed(signal_ctx: &SignalContext<'_>) -> zbus::Result<()>;

    /// Signal emitted when the overheat override turns on or off, with the
    /// hottest temperature (°C) and which sensor it came from ("CPU" or "GPU")
    #[dbus_interface(signal)]
    async fn overheat_changed(
        signal_ctx: &SignalContext<'_>,
        active: bool,
        temperature: f64,
        sensor: &str,
    ) -> zbus::Result<()>;

    /// Signal emitted when who controls the fans changes; `state` is an id
    /// from `alerts::fan_control`
    #[dbus_interface(signal)]
    async fn fan_control_changed(
        signal_ctx: &SignalContext<'_>,
        state: &str,
        detail: &str,
    ) -> zbus::Result<()>;

    /// Get all available fan curves
    async fn get_fan_curves(&self) -> zbus::fdo::Result<Vec<FanCurve>> {
        debug!("Getting fan curves");
        let config = self.config.lock().unwrap();
        Ok(config.curves.clone())
    }

    /// Get current fan curve
    async fn get_current_fan_curve(&self) -> zbus::fdo::Result<FanCurve> {
        debug!("Getting current fan curve");
        let config = self.config.lock().unwrap();
        if config.curves.is_empty() {
            return Err(Failed("No fan curves configured".into()));
        }
        let current_index =
            (*self.current_curve_index.lock().unwrap()).min(config.curves.len() - 1);
        Ok(config.curves[current_index].clone())
    }

    /// Set current fan curve by index
    async fn set_fan_curve(
        &self,
        index: u32,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Setting fan curve to index {}", index);
        let curve_name = {
            let config = self.config.lock().unwrap();
            let curve = config
                .curves
                .get(index as usize)
                .ok_or_else(|| Failed("Invalid fan curve index".into()))?;
            *self.current_curve_index.lock().unwrap() = index as usize;
            curve.name().to_string()
        };

        info!("Fan curve set to: {}", curve_name);
        emit_curve_changed(&ctxt).await;
        Ok(())
    }

    /// Set fan curve by name
    async fn set_fan_curve_by_name(
        &self,
        name: &str,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Setting fan curve to name: {}", name);
        let index = self.curve_index(name)?;
        *self.current_curve_index.lock().unwrap() = index;

        info!("Fan curve set to: {}", name);
        emit_curve_changed(&ctxt).await;
        Ok(())
    }

    /// Set the persistent default fan curve (survives reboots) and activate it now.
    async fn set_default_fan_curve(
        &self,
        name: &str,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Setting default fan curve to: {}", name);
        let index = self.curve_index(name)?;
        self.config.lock().unwrap().default_curve_index = Some(index);
        *self.current_curve_index.lock().unwrap() = index;
        self.save()?;

        info!("Default fan curve set to '{}'", name);
        emit_curve_changed(&ctxt).await;
        Ok(())
    }

    /// Get the persisted default fan curve (used on daemon startup / reboot)
    async fn get_default_fan_curve(&self) -> zbus::fdo::Result<FanCurve> {
        let config = self.config.lock().unwrap();
        if config.curves.is_empty() {
            return Err(Failed("No fan curves configured".into()));
        }
        let index = config
            .default_curve_index
            .unwrap_or(0)
            .min(config.curves.len() - 1);
        Ok(config.curves[index].clone())
    }

    /// Add a fan curve point. `duty` is a percentage (0-100) at the DBus edge;
    /// it is stored internally in ten-thousandths (0-10000).
    async fn add_fan_curve_point(
        &self,
        temp: i16,
        duty: u16,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Adding fan curve point: {}°C -> {}%", temp, duty);

        if !(0..=100).contains(&temp) || duty > 100 {
            return Err(Failed("Invalid fan curve point values".into()));
        }

        {
            let mut config = self.config.lock().unwrap();
            let index = *self.current_curve_index.lock().unwrap();
            let curve = config
                .curves
                .get_mut(index)
                .ok_or_else(|| Failed("Invalid current fan curve index".into()))?;
            if curve.is_builtin() {
                return Err(Failed(BUILTIN_READ_ONLY.into()));
            }
            curve.add_point(temp, duty * 100);
        }
        self.save()?;

        info!("Added fan curve point: {}°C -> {}%", temp, duty);
        emit_curve_changed(&ctxt).await;
        Ok(())
    }

    /// Remove last fan curve point
    async fn remove_fan_curve_point(
        &self,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Removing last fan curve point");

        {
            let mut config = self.config.lock().unwrap();
            let index = *self.current_curve_index.lock().unwrap();
            let curve = config
                .curves
                .get_mut(index)
                .ok_or_else(|| Failed("Invalid current fan curve index".into()))?;
            if curve.is_builtin() {
                return Err(Failed(BUILTIN_READ_ONLY.into()));
            }
            curve
                .remove_last_point()
                .ok_or_else(|| Failed("No points to remove".into()))?;
        }
        self.save()?;

        info!("Removed last fan curve point");
        emit_curve_changed(&ctxt).await;
        Ok(())
    }

    /// Save configuration
    async fn save_config(&self) -> zbus::fdo::Result<()> {
        self.save()
    }

    /// Replace the daemon's in-memory curves with a full config from a client
    /// (e.g. the GUI), persist it, and select `current_name` as the active curve.
    /// `default_index` of -1 means "leave default unchanged / none".
    async fn set_config(
        &self,
        curves: Vec<FanCurve>,
        default_index: i32,
        current_name: &str,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        if curves.is_empty() {
            return Err(Failed("Cannot set an empty curve list".into()));
        }
        // Built-in profiles are read-only: whatever a client sends for them,
        // they keep their factory definitions.
        let curves: Vec<FanCurve> = curves
            .into_iter()
            .map(|c| FanCurve::factory_by_name(c.name()).unwrap_or(c))
            .collect();

        let current_pos = curves
            .iter()
            .position(|c| c.name() == current_name)
            .ok_or_else(|| {
                Failed(format!(
                    "Current curve not found in provided config: {}",
                    current_name
                ))
            })?;

        let default = if default_index < 0 {
            None
        } else if (default_index as usize) < curves.len() {
            Some(default_index as usize)
        } else {
            return Err(Failed("Invalid default curve index".into()));
        };

        {
            let mut config = self.config.lock().unwrap();
            let mut index = self.current_curve_index.lock().unwrap();
            // SetConfig carries curves only; keep the channel assignments.
            let channels = std::mem::take(&mut config.channels);
            *config = FanCurveConfig {
                curves,
                default_curve_index: default,
                channels,
            };
            *index = current_pos;
        }
        self.save()?;

        info!(
            "Config replaced via DBus; active curve: {} (index {})",
            current_name, current_pos
        );
        emit_curve_changed(&ctxt).await;
        Ok(())
    }

    /// Live status from the control loop: (temperature_c, duty_0_10000, pwm_0_255, fan_speeds)
    async fn get_status(&self) -> zbus::fdo::Result<(f64, u16, u8, Vec<(u8, u16, String)>)> {
        let st = self.status.lock().unwrap();
        Ok((st.temperature, st.duty, st.pwm, st.fan_speeds.clone()))
    }

    /// Latest temperatures: (cpu_available, cpu_c, gpu_state, gpu_c), where
    /// gpu_state is "ok", "asleep" (powered down; not woken to read it) or
    /// "unavailable"
    async fn get_temperatures(&self) -> (bool, f64, String, f64) {
        let st = self.status.lock().unwrap();
        let (gpu_state, gpu_c) = match st.gpu_temp {
            Some(GpuReading::Value(t)) => ("ok", f64::from(t)),
            Some(GpuReading::Asleep) => ("asleep", 0.0),
            _ => ("unavailable", 0.0),
        };
        (
            st.cpu_temp.is_some(),
            f64::from(st.cpu_temp.unwrap_or_default()),
            gpu_state.to_string(),
            gpu_c,
        )
    }

    /// Thelio Io fan channels: [(channel, label, source, synced_source,
    /// duty_0_10000)]. `source` is the temperature the fan follows now;
    /// `synced_source` the one it follows when the active profile isn't
    /// independent. Sources are "Cpu", "Gpu" or "Hottest". Empty when no
    /// Thelio Io board is driven.
    async fn get_channels(&self) -> Vec<(u8, String, TempSource, TempSource, u16)> {
        self.status.lock().unwrap().channels.clone()
    }

    /// Set the temperature each listed channel follows when the active
    /// profile isn't independent. Unlisted channels are unchanged.
    async fn set_channels(&self, channels: Vec<(u8, TempSource)>) -> zbus::fdo::Result<()> {
        if let Some((number, _)) = channels.iter().find(|(n, _)| !(1..=4).contains(n)) {
            return Err(Failed(format!("Invalid channel: {}", number)));
        }

        {
            let mut config = self.config.lock().unwrap();
            for (number, source) in channels {
                match config.channels.iter_mut().find(|a| a.channel == number) {
                    Some(a) => a.source = source,
                    None => config.channels.push(ChannelAssignment {
                        channel: number,
                        source,
                        curve: None,
                    }),
                }
            }
        }

        self.save()?;
        info!("Fan channel temperature sources updated");
        Ok(())
    }

    /// Overheat override: (active, hottest_c, sensor)
    async fn get_overheat(&self) -> (bool, f64, String) {
        self.status.lock().unwrap().overheat.clone()
    }

    /// Who controls the fans: (state, detail); `state` is an id from
    /// `alerts::fan_control`
    async fn get_fan_control_state(&self) -> (String, String) {
        self.status.lock().unwrap().fan_control.clone()
    }

    /// Reload configuration from disk, replacing the in-memory state
    async fn reload_config(
        &self,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Reloading configuration from disk");

        let new_config = Self::load_config().map_err(|e| Failed(e.to_string()))?;

        {
            let mut config = self.config.lock().unwrap();
            let mut current_index = self.current_curve_index.lock().unwrap();
            *current_index = new_config
                .default_curve_index
                .unwrap_or(0)
                .min(new_config.curves.len().saturating_sub(1));
            *config = new_config;
        }

        info!("Configuration reloaded from disk");
        emit_curve_changed(&ctxt).await;
        Ok(())
    }
}

async fn emit_curve_changed(ctxt: &SignalContext<'_>) {
    if let Err(e) = FanCurveDaemon::fan_curve_changed(ctxt).await {
        warn!("Failed to emit FanCurveChanged signal: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thelio_fans_are_left_to_system76_power_once_released() {
        assert!(!restore_fans_on_exit(&ClaimOutcome::Granted, true));
        // Release failed: nothing resumes them, so the daemon does
        assert!(restore_fans_on_exit(&ClaimOutcome::Granted, false));
    }

    #[test]
    fn fans_the_daemon_never_wrote_are_left_alone() {
        for claim in [
            ClaimOutcome::HeldByOther,
            ClaimOutcome::NotRunning,
            ClaimOutcome::Unsupported,
            ClaimOutcome::Failed("polkit".to_string()),
        ] {
            assert!(!restore_fans_on_exit(&claim, false));
        }
    }
}
