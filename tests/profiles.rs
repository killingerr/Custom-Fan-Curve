//! Per-fan profiles end to end: a client sets up profiles over DBus the way the
//! GUI does, and the daemon is restarted from its saved config to check what
//! survives. Each test runs a real daemon on a private peer-to-peer DBus
//! connection with its own config file, so no system bus or root is needed.

use fan_curve_app::channels::{channel_duty, TempSource};
use fan_curve_app::daemon::FanCurveDaemon;
use fan_curve_app::fan::{FanChannelCurve, FanCurve, FanCurveConfig, FanPoint};
use fan_curve_app::{DBUS_INTERFACE_NAME, DBUS_OBJECT_PATH, DBUS_SERVICE_NAME};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use zbus::{Connection, ConnectionBuilder, Guid, Proxy};

/// The daemon finds its config through FAN_CURVE_APP_CONFIG, which is
/// process-wide, so tests that set it take turns.
static CONFIG_ENV: Mutex<()> = Mutex::new(());

/// A config file of its own for one test, removed afterwards.
struct TestConfig {
    dir: PathBuf,
    _env: MutexGuard<'static, ()>,
}

impl TestConfig {
    fn new(name: &str) -> Self {
        let env = CONFIG_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "fan_curve_profiles_{}_{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FAN_CURVE_APP_CONFIG", dir.join("config.json"));
        Self { dir, _env: env }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    /// The config as the daemon saved it.
    fn saved(&self) -> FanCurveConfig {
        FanCurveConfig::load_from_file(&self.path()).expect("saved config loads")
    }
}

impl Drop for TestConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A running daemon and a client connected to it. Dropping it stops the
/// daemon, like quitting the app.
struct Daemon {
    _server: Connection,
    _client: Connection,
    proxy: Proxy<'static>,
}

impl Daemon {
    /// Start a daemon from the current config file, as at boot.
    async fn start() -> Self {
        let daemon = FanCurveDaemon::new().expect("daemon starts");
        let (server_stream, client_stream) = tokio::net::UnixStream::pair().unwrap();
        let guid = Guid::generate();
        let (server, client) = tokio::try_join!(
            ConnectionBuilder::unix_stream(server_stream)
                .server(&guid)
                .p2p()
                .serve_at(DBUS_OBJECT_PATH, daemon)
                .unwrap()
                .build(),
            ConnectionBuilder::unix_stream(client_stream).p2p().build(),
        )
        .expect("peer-to-peer connection");
        let proxy = Proxy::new(
            &client,
            DBUS_SERVICE_NAME,
            DBUS_OBJECT_PATH,
            DBUS_INTERFACE_NAME,
        )
        .await
        .unwrap();
        Self {
            _server: server,
            _client: client,
            proxy,
        }
    }

    async fn curves(&self) -> Vec<FanCurve> {
        self.proxy.call("GetFanCurves", &()).await.unwrap()
    }

    async fn current(&self) -> FanCurve {
        self.proxy.call("GetCurrentFanCurve", &()).await.unwrap()
    }

    async fn default(&self) -> FanCurve {
        self.proxy.call("GetDefaultFanCurve", &()).await.unwrap()
    }

    /// SetConfig as the GUI sends it after an edit, Apply or Set as Default.
    async fn set_config(&self, curves: &[FanCurve], default_index: Option<usize>, current: &str) {
        let default_index = default_index.map_or(-1, |i| i as i32);
        self.proxy
            .call::<_, _, ()>("SetConfig", &(curves, default_index, current))
            .await
            .expect("daemon accepts the config");
    }
}

fn points(points: &[(i16, u16)]) -> Vec<FanPoint> {
    points.iter().map(|&(t, d)| FanPoint::new(t, d)).collect()
}

/// A profile driving each Thelio Io fan from its own curve and sensor. At
/// CPU 60°C / GPU 80°C the fans should run at 40%, 70%, 80% and 25%.
fn split_profile(name: &str) -> FanCurve {
    let mut profile = FanCurve::new(name.to_string());
    profile.add_point(0, 5000);
    profile.set_independent(true);
    // CPU fan: CPU 60°C, halfway from 20% at 40°C to 60% at 80°C -> 40%
    profile.set_fan(FanChannelCurve::new(
        1,
        TempSource::Cpu,
        &points(&[(40, 2000), (80, 6000)]),
    ));
    // Intake: hottest (GPU 80°C), halfway from 40% at 60°C to 100% at 100°C -> 70%
    profile.set_fan(FanChannelCurve::new(
        2,
        TempSource::Hottest,
        &points(&[(60, 4000), (100, 10000)]),
    ));
    // GPU fan: GPU 80°C is exactly a point -> 80%
    profile.set_fan(FanChannelCurve::new(
        3,
        TempSource::Gpu,
        &points(&[(30, 3000), (80, 8000), (90, 10000)]),
    ));
    // Aux: flat 25%
    profile.set_fan(FanChannelCurve::new(
        4,
        TempSource::Cpu,
        &points(&[(0, 2500)]),
    ));
    profile
}

/// Each fan's duty (0-10000) under `profile` in `config` at CPU 60°C, GPU 80°C.
fn fan_duties(config: &FanCurveConfig, profile: &str) -> Vec<Option<u16>> {
    let index = config
        .curves
        .iter()
        .position(|c| c.name() == profile)
        .expect("profile exists");
    (1..=4)
        .map(|channel| channel_duty(config, index, true, channel, Some(60.0), Some(80.0)))
        .collect()
}

const SPLIT_DUTIES: [Option<u16>; 4] = [Some(4000), Some(7000), Some(8000), Some(2500)];

#[tokio::test]
async fn independent_fan_curves_save_into_one_profile() {
    let config = TestConfig::new("save");
    let daemon = Daemon::start().await;

    let mut curves = daemon.curves().await;
    let split = split_profile("Split Fans");
    curves.push(split.clone());
    daemon.set_config(&curves, None, "Split Fans").await;

    // The daemon is running the profile with all four fan curves
    assert_eq!(daemon.current().await, split);

    // and saved them, together, in that one profile
    let saved = config.saved();
    let saved_split = saved
        .curves
        .iter()
        .find(|c| c.name() == "Split Fans")
        .expect("profile saved");
    assert_eq!(saved_split, &split);
    assert!(saved_split.is_independent());
    assert_eq!(saved_split.fans().len(), 4);

    // Every fan follows its own curve and sensor
    assert_eq!(fan_duties(&saved, "Split Fans"), SPLIT_DUTIES);
}

#[tokio::test]
async fn default_profile_persists_across_restart() {
    let config = TestConfig::new("default");
    let daemon = Daemon::start().await;

    // Set as Default in the GUI: the profile's index goes in SetConfig
    let mut curves = daemon.curves().await;
    curves.push(split_profile("Split Fans"));
    let split_index = curves.len() - 1;
    daemon
        .set_config(&curves, Some(split_index), "Split Fans")
        .await;
    assert_eq!(daemon.default().await.name(), "Split Fans");
    drop(daemon);

    // Restart: the daemon comes back on the default with every fan curve
    let daemon = Daemon::start().await;
    assert_eq!(daemon.current().await, split_profile("Split Fans"));
    assert_eq!(daemon.default().await, split_profile("Split Fans"));
    assert_eq!(config.saved().default_curve_index, Some(split_index));
    assert_eq!(fan_duties(&config.saved(), "Split Fans"), SPLIT_DUTIES);

    // and it survives a second restart too
    drop(daemon);
    let daemon = Daemon::start().await;
    assert_eq!(daemon.current().await.name(), "Split Fans");
}

#[tokio::test]
async fn set_default_fan_curve_by_name_persists_across_restart() {
    let _config = TestConfig::new("default_by_name");
    let daemon = Daemon::start().await;
    let mut curves = daemon.curves().await;
    curves.push(split_profile("Split Fans"));
    daemon.set_config(&curves, None, "Standard").await;

    // The CLI's path: SetDefaultFanCurve by name
    daemon
        .proxy
        .call::<_, _, ()>("SetDefaultFanCurve", &("Split Fans",))
        .await
        .unwrap();
    assert_eq!(daemon.current().await.name(), "Split Fans");
    drop(daemon);

    let daemon = Daemon::start().await;
    assert_eq!(daemon.current().await, split_profile("Split Fans"));
}

#[tokio::test]
async fn applied_profile_lasts_only_until_restart() {
    let config = TestConfig::new("apply");
    let daemon = Daemon::start().await;
    let default_name = daemon.default().await.name().to_string();

    // Apply in the GUI: switch to the profile without changing the default
    let mut curves = daemon.curves().await;
    curves.push(split_profile("Split Fans"));
    daemon.set_config(&curves, None, "Split Fans").await;
    assert_eq!(daemon.current().await.name(), "Split Fans");
    drop(daemon);

    // After a restart the default is back in charge, and the applied
    // profile is still saved for later
    let daemon = Daemon::start().await;
    assert_eq!(daemon.current().await.name(), default_name);
    assert!(config.saved().curves.contains(&split_profile("Split Fans")));
}

#[tokio::test]
async fn save_as_new_profile_from_a_built_in_gives_an_editable_copy() {
    let config = TestConfig::new("save_as_new");
    let daemon = Daemon::start().await;
    let mut curves = daemon.curves().await;

    // Save as New Profile from built-in Standard, then give the copy's GPU
    // fan its own curve and make it the default
    let mut copy = FanCurve::standard();
    copy.set_name("My Standard".to_string());
    copy.set_independent(true);
    copy.set_fan(FanChannelCurve::new(
        3,
        TempSource::Gpu,
        &points(&[(0, 9000)]),
    ));
    curves.push(copy.clone());
    daemon
        .set_config(&curves, Some(curves.len() - 1), "My Standard")
        .await;
    drop(daemon);

    let daemon = Daemon::start().await;
    // The copy kept its per-fan edits and is the default
    assert_eq!(daemon.current().await, copy);
    assert!(config.saved().curves.contains(&copy));
    // and Standard itself is untouched
    let restarted = daemon.curves().await;
    let standard = restarted.iter().find(|c| c.name() == "Standard").unwrap();
    assert_eq!(standard, &FanCurve::standard());
}

/// Every built-in profile at its factory definition.
fn assert_builtins_are_factory(curves: &[FanCurve]) {
    for name in ["Standard", "Threadripper 2", "HEDT", "Xeon"] {
        let curve = curves.iter().find(|c| c.name() == name).unwrap();
        assert_eq!(
            Some(curve),
            FanCurve::factory_by_name(name).as_ref(),
            "{} changed",
            name
        );
    }
}

#[tokio::test]
async fn built_in_profiles_cannot_be_saved_over() {
    let config = TestConfig::new("builtin_read_only");
    let daemon = Daemon::start().await;
    let mut curves = daemon.curves().await;

    // A client sends synced edits to Standard and HEDT, and independent fan
    // curves for Xeon and Threadripper 2
    for curve in curves.iter_mut() {
        match curve.name() {
            "Standard" | "HEDT" => curve.add_point(30, 10000),
            "Xeon" | "Threadripper 2" => {
                curve.set_independent(true);
                curve.set_fan(FanChannelCurve::new(
                    1,
                    TempSource::Cpu,
                    &points(&[(0, 10000)]),
                ));
            }
            _ => {}
        }
    }
    daemon.set_config(&curves, None, "Xeon").await;

    // None of it takes, not even for this session: the fans keep following
    // the factory curves
    assert_builtins_are_factory(&daemon.curves().await);
    assert_eq!(
        Some(daemon.current().await),
        FanCurve::factory_by_name("Xeon")
    );
    let saved = config.saved();
    assert_builtins_are_factory(&saved.curves);
    // CPU fan at CPU 60°C; the others follow the GPU or hottest, 80°C
    let xeon = |t| Some(FanCurve::xeon().calculate_duty_for_temperature_celsius(t));
    assert_eq!(
        fan_duties(&saved, "Xeon"),
        [xeon(60.0), xeon(80.0), xeon(80.0), xeon(80.0)]
    );

    // The CLI's point editing is refused on built-ins
    let add = daemon
        .proxy
        .call::<_, _, ()>("AddFanCurvePoint", &(30i16, 100u16))
        .await;
    let remove = daemon
        .proxy
        .call::<_, _, ()>("RemoveFanCurvePoint", &())
        .await;
    for result in [add, remove] {
        let error = result.expect_err("built-in edit refused").to_string();
        assert!(error.contains("read-only"), "{}", error);
    }
    assert_builtins_are_factory(&daemon.curves().await);

    // and they're still factory after a restart
    drop(daemon);
    let daemon = Daemon::start().await;
    assert_builtins_are_factory(&daemon.curves().await);
}

#[tokio::test]
async fn syncing_a_profile_keeps_its_fan_curves_for_later() {
    let config = TestConfig::new("sync_toggle");
    let daemon = Daemon::start().await;
    let mut curves = daemon.curves().await;
    let mut split = split_profile("Split Fans");
    split.set_independent(false);
    curves.push(split.clone());
    let split_index = curves.len() - 1;
    daemon
        .set_config(&curves, Some(split_index), "Split Fans")
        .await;
    drop(daemon);

    // Synced: every fan follows the profile's own points (flat 50%)
    let daemon = Daemon::start().await;
    let saved = config.saved();
    assert_eq!(fan_duties(&saved, "Split Fans"), [Some(5000); 4]);

    // Switching back to independent restores the four fan curves
    let mut curves = daemon.curves().await;
    let current = daemon.current().await;
    assert_eq!(current.fans().len(), 4);
    curves[split_index].set_independent(true);
    daemon
        .set_config(&curves, Some(split_index), "Split Fans")
        .await;
    drop(daemon);

    let _daemon = Daemon::start().await;
    assert_eq!(fan_duties(&config.saved(), "Split Fans"), SPLIT_DUTIES);
}
