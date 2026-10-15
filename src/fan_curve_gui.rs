use crate::channels::{GpuReading, GpuTemp, TempSource};
use crate::errors::Result;
use crate::fan::{FanChannelCurve, FanCurve, FanCurveConfig, FanPoint};
use crate::fan_monitor::FanMonitor;
use eframe::egui;

pub struct FanCurveApp {
    fan_curves: Vec<FanCurve>,
    current_curve_index: usize,
    default_curve_index: Option<usize>,
    status_message: Option<String>,
    new_curve_name: String,
    show_save_dialog: bool,
    fan_monitor: FanMonitor,
    /// Proxy to the fan curve daemon; when present, the daemon owns the
    /// hardware and the GUI only sends commands over DBus
    daemon: Option<zbus::blocking::Proxy<'static>>,
    last_applied_curve_index: Option<usize>,
    current_fan_data: Option<crate::fan_monitor::FanDataPoint>,
    last_fan_data_update: std::time::Instant,
    show_add_point_dialog: bool,
    new_point_temp: String,
    new_point_duty: String,
    show_edit_point_dialog: bool,
    edit_point_index: Option<usize>,
    edit_point_temp: String,
    edit_point_duty: String,
    /// Daemon-reported problems shown as banners: overheat override and fan
    /// control state, as (summary, body)
    overheat_alert: Option<crate::alerts::Message>,
    fan_control_alert: Option<crate::alerts::Message>,
    /// CPU model name, read once at startup
    cpu_model: Option<String>,
    /// GPU model names, read once at startup
    gpu_models: Vec<crate::gpu_info::GpuInfo>,
    /// Latest CPU and GPU temperatures for the Live Fan Data section
    cpu_temp: Option<f32>,
    gpu_temp: Option<GpuReading>,
    gpu_usage: Option<GpuReading>,
    /// Local GPU reader: GPU usage, and temperature when the daemon can't
    /// report it
    local_gpu: Option<GpuTemp>,
    /// Thelio Io fan channels the daemon drives; empty without a board
    fans: Vec<FanSetting>,
    /// Index into `fans` of the tab shown for an independent profile
    selected_fan: usize,
    /// Curve the open add/edit point dialog changes
    point_target: PointTarget,
}

/// One Thelio Io fan channel as shown in the GUI.
struct FanSetting {
    number: u8,
    label: String,
    /// Temperature the fan follows when the profile isn't independent
    synced_source: TempSource,
    /// Current duty (0-10000) reported by the daemon
    duty: u16,
}

/// Curve a point edit applies to.
#[derive(Clone, Copy, PartialEq)]
enum PointTarget {
    /// The selected profile's points
    Profile,
    /// The selected profile's own curve for a fan channel
    Fan(u8),
}

/// What the user clicked in a list of curve points.
enum PointAction {
    Edit(usize),
    Remove(usize),
}

/// Fan channels from the daemon, or None if it can't report them.
fn fetch_channels(proxy: &zbus::blocking::Proxy<'static>) -> Option<Vec<FanSetting>> {
    let channels = proxy
        .call::<_, _, Vec<(u8, String, TempSource, TempSource, u16)>>("GetChannels", &())
        .ok()?;
    Some(
        channels
            .into_iter()
            .map(|(number, label, _source, synced_source, duty)| FanSetting {
                number,
                label,
                synced_source,
                duty,
            })
            .collect(),
    )
}

/// List `points` with Edit and Remove buttons.
fn show_points(ui: &mut egui::Ui, points: &[FanPoint]) -> Option<PointAction> {
    let mut action = None;
    for (i, point) in points.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("Point {}: ", i + 1));
            ui.label(format!("{}°C -> {}%", point.temp, point.duty_percent()));

            ui.add_space(10.0);

            if ui.button("Edit").clicked() {
                action = Some(PointAction::Edit(i));
            }

            ui.add_space(5.0);

            if ui.button("Remove").clicked() {
                action = Some(PointAction::Remove(i));
            }
        });
    }
    action
}

/// Try to connect to the fan curve daemon on the system bus.
/// Returns None if the bus is unreachable or the daemon isn't running.
fn connect_daemon() -> Option<zbus::blocking::Proxy<'static>> {
    let connection = zbus::blocking::Connection::system().ok()?;

    // Check name ownership first so later calls fail fast instead of hanging
    let dbus = zbus::blocking::fdo::DBusProxy::new(&connection).ok()?;
    let name = zbus::names::BusName::try_from(crate::DBUS_SERVICE_NAME).ok()?;
    if !dbus.name_has_owner(name).ok()? {
        return None;
    }

    zbus::blocking::Proxy::new(
        &connection,
        crate::DBUS_SERVICE_NAME,
        crate::DBUS_OBJECT_PATH,
        crate::DBUS_INTERFACE_NAME,
    )
    .ok()
}

/// Load curves from the daemon when connected, otherwise from the local config file.
fn load_initial_curves(
    daemon: Option<&zbus::blocking::Proxy<'static>>,
) -> (Vec<FanCurve>, Option<usize>, usize) {
    if let Some(proxy) = daemon {
        if let Ok(curves) = proxy.call::<_, _, Vec<FanCurve>>("GetFanCurves", &()) {
            if !curves.is_empty() {
                let default_index = proxy
                    .call::<_, _, FanCurve>("GetDefaultFanCurve", &())
                    .ok()
                    .and_then(|d| curves.iter().position(|c| c.name() == d.name()));
                let current_index = proxy
                    .call::<_, _, FanCurve>("GetCurrentFanCurve", &())
                    .ok()
                    .and_then(|c| curves.iter().position(|x| x.name() == c.name()))
                    .or(default_index)
                    .unwrap_or(0);
                return (curves, default_index.or(Some(0)), current_index);
            }
        }
    }

    let config_path = FanCurveConfig::get_config_path();
    let config = if config_path.exists() {
        FanCurveConfig::load_from_file(&config_path).unwrap_or_else(|_| FanCurveConfig::new())
    } else {
        FanCurveConfig::new()
    };
    let default = config.default_curve_index.or(Some(0));
    let current = default
        .unwrap_or(0)
        .min(config.curves.len().saturating_sub(1));
    (config.curves, default, current)
}

impl FanCurveApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Prefer the daemon's persisted config when available so the GUI
        // matches what survives reboot; fall back to the user config file.
        let daemon = connect_daemon();
        let (fan_curves, default_curve_index, current_curve_index) =
            load_initial_curves(daemon.as_ref());

        let mut fan_monitor = FanMonitor::new();
        if let Err(e) = fan_monitor.initialize() {
            eprintln!(
                "Warning: Failed to initialize CPU temperature detection: {}",
                e
            );
            eprintln!("Falling back to simulation mode");
        }

        let fans = daemon.as_ref().and_then(fetch_channels).unwrap_or_default();

        if daemon.is_some() {
            println!("Connected to fan curve daemon; daemon controls the fans");
        } else {
            println!("Fan curve daemon not running; GUI controls the fans directly");
        }

        Self {
            fan_curves,
            current_curve_index,
            default_curve_index,
            status_message: None,
            new_curve_name: String::new(),
            show_save_dialog: false,
            fan_monitor,
            daemon,
            current_fan_data: None,
            last_fan_data_update: std::time::Instant::now(),
            show_add_point_dialog: false,
            new_point_temp: String::new(),
            new_point_duty: String::new(),
            show_edit_point_dialog: false,
            edit_point_index: None,
            edit_point_temp: String::new(),
            edit_point_duty: String::new(),
            last_applied_curve_index: None,
            overheat_alert: None,
            fan_control_alert: None,
            cpu_model: crate::cpu_temp::cpu_model_name(),
            gpu_models: crate::gpu_info::detect_gpus(),
            cpu_temp: None,
            gpu_temp: None,
            gpu_usage: None,
            local_gpu: None,
            fans,
            selected_fan: 0,
            point_target: PointTarget::Profile,
        }
    }

    fn save_config(&self) -> Result<()> {
        let config_path = FanCurveConfig::get_config_path();

        // Ensure the directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                eprintln!("Failed to create config directory: {}", e);
                e
            })?;
        }

        // Channel settings live in the daemon's config; keep any in this file.
        let channels = FanCurveConfig::load_from_file(&config_path)
            .map(|c| c.channels)
            .unwrap_or_default();
        let config = FanCurveConfig {
            curves: self.fan_curves.clone(),
            default_curve_index: self.default_curve_index,
            channels,
        };

        // Create a temporary file first, then rename for atomic operation
        let temp_path = config_path.with_extension("tmp");

        // Save to temporary file
        config.save_to_file(&temp_path).map_err(|e| {
            eprintln!("Failed to save config to temp file: {}", e);
            e
        })?;

        // Atomically rename temp file to final location
        std::fs::rename(&temp_path, &config_path).map_err(|e| {
            eprintln!("Failed to rename temp config file: {}", e);
            // Try to clean up temp file
            let _ = std::fs::remove_file(&temp_path);
            e
        })?;

        println!(
            "Configuration saved successfully to: {}",
            config_path.display()
        );
        Ok(())
    }

    fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
    }

    /// Push the local curve list to the daemon so its control loop uses it.
    /// Returns Ok(true) if the push succeeded, Ok(false) if no daemon is connected.
    fn push_config_to_daemon(&mut self) -> std::result::Result<bool, String> {
        if self.daemon.is_none() {
            self.daemon = connect_daemon();
        }

        let Some(ref proxy) = self.daemon else {
            return Ok(false);
        };

        let curves = self.fan_curves.clone();
        let default_index = self.default_curve_index.map(|i| i as i32).unwrap_or(-1);
        let current_name = self.fan_curves[self.current_curve_index].name().to_string();

        proxy
            .call::<_, _, ()>("SetConfig", &(curves, default_index, current_name.as_str()))
            .map_err(|e| e.to_string())?;

        Ok(true)
    }

    /// Send the temperature each fan follows in synced profiles to the
    /// daemon, which saves them.
    fn push_channels_to_daemon(&mut self) {
        let Some(ref proxy) = self.daemon else {
            self.set_status("Daemon not running — fan settings can't be applied.".to_string());
            return;
        };
        let channels: Vec<(u8, TempSource)> = self
            .fans
            .iter()
            .map(|f| (f.number, f.synced_source))
            .collect();
        match proxy.call::<_, _, ()>("SetChannels", &(channels,)) {
            Ok(()) => self.set_status("Fan temperature sources saved".to_string()),
            Err(e) => self.set_status(format!("Daemon rejected fan settings: {}", e)),
        }
    }

    fn current_profile(&self) -> &FanCurve {
        &self.fan_curves[self.current_curve_index]
    }

    /// The selected profile drives each fan with its own curve.
    fn profile_is_independent(&self) -> bool {
        !self.fans.is_empty() && self.current_profile().is_independent()
    }

    /// Switch the selected profile between every fan following its points and
    /// each fan following its own curve. Fans without a curve yet start from
    /// the profile's points and the temperature they follow when synced.
    fn set_independent_fans(&mut self, independent: bool) {
        let profile = &mut self.fan_curves[self.current_curve_index];
        profile.set_independent(independent);
        if independent {
            let points = profile.points().to_vec();
            for fan in &self.fans {
                if profile.fan(fan.number).is_none() {
                    profile.set_fan(FanChannelCurve::new(fan.number, fan.synced_source, &points));
                }
            }
        }
        self.auto_save_config();
    }

    /// The selected profile's curve for fan `channel`, created empty if missing.
    fn fan_curve_mut(&mut self, channel: u8) -> &mut FanChannelCurve {
        let source = self
            .fans
            .iter()
            .find(|f| f.number == channel)
            .map_or(TempSource::Hottest, |f| f.synced_source);
        let profile = &mut self.fan_curves[self.current_curve_index];
        if profile.fan(channel).is_none() {
            profile.set_fan(FanChannelCurve::new(channel, source, &[]));
        }
        profile.fan_mut(channel).expect("fan curve was just added")
    }

    fn target_points(&self) -> &[FanPoint] {
        match self.point_target {
            PointTarget::Profile => self.current_profile().points(),
            PointTarget::Fan(channel) => self
                .current_profile()
                .fan(channel)
                .map_or(&[][..], |f| f.points()),
        }
    }

    fn add_target_point(&mut self, temp: i16, duty: u16) {
        match self.point_target {
            PointTarget::Profile => self.fan_curves[self.current_curve_index].add_point(temp, duty),
            PointTarget::Fan(channel) => self.fan_curve_mut(channel).add_point(temp, duty),
        }
    }

    fn remove_target_point(&mut self, index: usize) -> Option<FanPoint> {
        match self.point_target {
            PointTarget::Profile => self.fan_curves[self.current_curve_index].remove_point(index),
            PointTarget::Fan(channel) => self.fan_curve_mut(channel).remove_point(index),
        }
    }

    fn apply_point_action(&mut self, target: PointTarget, action: PointAction) {
        self.point_target = target;
        match action {
            PointAction::Edit(i) => {
                let point = self.target_points()[i].clone();
                self.edit_point_temp = point.temp.to_string();
                self.edit_point_duty = point.duty_percent().to_string();
                self.edit_point_index = Some(i);
                self.show_edit_point_dialog = true;
            }
            PointAction::Remove(i) => {
                if let Some(removed) = self.remove_target_point(i) {
                    self.set_status(format!(
                        "Removed point {}: {}°C -> {}%",
                        i + 1,
                        removed.temp,
                        removed.duty_percent()
                    ));
                    self.auto_save_config();
                }
            }
        }
    }

    fn open_add_point_dialog(&mut self, target: PointTarget) {
        self.point_target = target;
        self.show_add_point_dialog = true;
        self.new_point_temp = "50".to_string();
        self.new_point_duty = "50".to_string();
    }

    /// Profile selection and its points, which every fan follows when synced.
    fn show_profile_editor(&mut self, ui: &mut egui::Ui) {
        // Current fan profile display
        let current_profile = self.fan_curves[self.current_curve_index].name();
        ui.label(format!("Current Profile: {}", current_profile));
        if self.fan_curves[self.current_curve_index].is_builtin() {
            ui.colored_label(
                egui::Color32::from_rgb(180, 140, 40),
                "Built-in profile: edits apply for this session only. Save as New Profile to keep them.",
            );
        }

        // Fan curve selection
        egui::ComboBox::from_label("Select Fan Curve")
            .selected_text(self.fan_curves[self.current_curve_index].name())
            .show_ui(ui, |ui| {
                for (index, curve) in self.fan_curves.iter().enumerate() {
                    let mut text = curve.name().to_string();
                    if Some(index) == self.default_curve_index {
                        text += " (Default)";
                    }
                    ui.selectable_value(&mut self.current_curve_index, index, text);
                }
            });

        if !self.fans.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Fans:");
                let current = self.current_profile().is_independent();
                let mut independent = current;
                ui.radio_value(&mut independent, false, "Sync all fans");
                ui.radio_value(&mut independent, true, "Control each fan independently");
                if independent != current {
                    self.set_independent_fans(independent);
                }
            });
        }

        ui.separator();
        if self.profile_is_independent() {
            self.show_fan_editor(ui);
        } else {
            if self.fans.is_empty() {
                ui.label("Fan Curve Points:");
            } else {
                ui.label("Fan Curve Points (all fans):");
            }

            if let Some(action) = show_points(ui, self.current_profile().points()) {
                self.apply_point_action(PointTarget::Profile, action);
            }

            ui.separator();

            // Add point button
            if ui.button("Add Point").clicked() {
                self.open_add_point_dialog(PointTarget::Profile);
            }
        }

        ui.separator();

        // Save as new profile button
        if ui.button("Save as New Profile").clicked() {
            self.show_save_dialog = true;
        }

        // Apply button — temporary for this session; reboot returns to default
        if ui.button("Apply Fan Curve").clicked() {
            if let Err(e) = self.save_config() {
                self.set_status(format!("Failed to save: {}", e));
            } else {
                let default_name = self
                    .default_curve_index
                    .and_then(|i| self.fan_curves.get(i))
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| "Standard".to_string());
                let name = self.fan_curves[self.current_curve_index].name().to_string();
                match self.push_config_to_daemon() {
                    Ok(true) => {
                        self.set_status(format!(
                            "Applied '{}' for this session. Reboot returns to default '{}'.",
                            name, default_name
                        ));
                    }
                    Ok(false) => {
                        self.set_status(
                            "Daemon not running — curve saved but fans are NOT controlled. Run: sudo ./scripts/install-daemon.sh"
                                .to_string(),
                        );
                    }
                    Err(e) => {
                        self.set_status(format!(
                            "Saved locally, but daemon rejected config: {}",
                            e
                        ));
                    }
                }
            }
        }

        // Set as default — persists across reboots via the daemon config
        if ui.button("Set as Default").clicked() {
            self.default_curve_index = Some(self.current_curve_index);
            let name = self.fan_curves[self.current_curve_index].name().to_string();
            if let Err(e) = self.save_config() {
                self.set_status(format!("Failed to save: {}", e));
            } else {
                match self.push_config_to_daemon() {
                    Ok(true) => self.set_status(format!(
                        "'{}' is now the default and will be used after reboot.",
                        name
                    )),
                    Ok(false) => self.set_status(format!(
                        "'{}' saved as default locally. Start the daemon so it persists across reboots.",
                        name
                    )),
                    Err(e) => self.set_status(format!(
                        "Saved locally, but daemon rejected default: {}",
                        e
                    )),
                }
            }
        }

        if !self.fans.is_empty() && !self.profile_is_independent() {
            ui.separator();
            ui.label("Temperature each fan follows:");
            self.show_fan_sources(ui);
        }
    }

    /// A temperature source picker and current duty for every fan.
    fn show_fan_sources(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        for fan in &mut self.fans {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", fan.label));
                changed |= source_picker(ui, ("synced", fan.number), &mut fan.synced_source);
                ui.label(format!("now {}%", fan.duty / 100));
            });
        }
        if changed {
            self.push_channels_to_daemon();
        }
    }

    /// One tab per fan, each with the selected profile's own temperature
    /// source and points for it.
    fn show_fan_editor(&mut self, ui: &mut egui::Ui) {
        self.selected_fan = self.selected_fan.min(self.fans.len() - 1);
        ui.horizontal(|ui| {
            for (i, fan) in self.fans.iter().enumerate() {
                ui.selectable_value(&mut self.selected_fan, i, &fan.label);
            }
        });
        ui.separator();

        let channel = self.fans[self.selected_fan].number;
        let label = self.fans[self.selected_fan].label.clone();
        let duty = self.fans[self.selected_fan].duty;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Follows:");
            let source = &mut self.fan_curve_mut(channel).source;
            changed |= source_picker(ui, ("fan", channel), source);
            ui.label(format!("now {}%", duty / 100));
        });

        // Replace this fan's points with another profile's curve for it, or
        // that profile's points when it isn't independent
        let mut copy_from = None;
        egui::ComboBox::from_id_salt("copy_profile_to_fan")
            .selected_text("Copy curve from profile...")
            .show_ui(ui, |ui| {
                for (index, curve) in self.fan_curves.iter().enumerate() {
                    if ui.selectable_label(false, curve.name()).clicked() {
                        copy_from = Some(index);
                    }
                }
            });
        if let Some(index) = copy_from {
            let other = &self.fan_curves[index];
            let points = match other.fan(channel) {
                Some(fan) if other.is_independent() && !fan.points().is_empty() => {
                    fan.points().to_vec()
                }
                _ => other.points().to_vec(),
            };
            self.fan_curve_mut(channel).set_points(&points);
            changed = true;
        }
        if changed {
            self.auto_save_config();
        }

        ui.separator();
        ui.label(format!("{} Curve Points:", label));
        let points = self
            .current_profile()
            .fan(channel)
            .map(|f| f.points().to_vec())
            .unwrap_or_default();
        if points.is_empty() {
            ui.colored_label(
                egui::Color32::GRAY,
                "No points: this fan follows the profile's all-fans points until you add one.",
            );
        }
        if let Some(action) = show_points(ui, &points) {
            self.apply_point_action(PointTarget::Fan(channel), action);
        }

        ui.separator();
        if ui.button("Add Point").clicked() {
            self.open_add_point_dialog(PointTarget::Fan(channel));
        }
    }

    /// Auto-save configuration with error handling
    fn auto_save_config(&mut self) {
        let editing_builtin = self.fan_curves[self.current_curve_index].is_builtin();

        if let Err(e) = self.save_config() {
            eprintln!("Auto-save failed: {}", e);
            self.set_status(format!("Auto-save failed: {}", e));
            return;
        }

        if editing_builtin {
            self.set_status(
                "Built-in profile changes are temporary. Use Save as New Profile to keep them."
                    .to_string(),
            );
            // Still push so Apply/session control can use the in-memory edits.
            let _ = self.push_config_to_daemon();
            return;
        }

        match self.push_config_to_daemon() {
            Ok(true) => self.set_status("Configuration saved and pushed to daemon".to_string()),
            Ok(false) => self.set_status("Configuration saved".to_string()),
            Err(e) => self.set_status(format!("Saved locally, but daemon push failed: {}", e)),
        }
    }
}

/// Temperature source drop-down; true if the user changed `source`.
fn source_picker(ui: &mut egui::Ui, id: impl std::hash::Hash, source: &mut TempSource) -> bool {
    let before = *source;
    egui::ComboBox::from_id_salt(id)
        .selected_text(source.label())
        .show_ui(ui, |ui| {
            for option in TempSource::ALL {
                ui.selectable_value(source, option, option.label());
            }
        });
    *source != before
}

/// Colour for a temperature reading in the Live Fan Data section.
fn temp_color(celsius: f32) -> egui::Color32 {
    if celsius > 70.0 {
        egui::Color32::RED
    } else if celsius > 50.0 {
        egui::Color32::YELLOW
    } else {
        egui::Color32::GREEN
    }
}

impl eframe::App for FanCurveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Always update live fan data every 1s
        if self.last_fan_data_update.elapsed() >= std::time::Duration::from_secs(1) {
            // Prefer telemetry from the daemon when connected
            let mut used_daemon_status = false;
            if let Some(ref proxy) = self.daemon {
                if let Ok((temp, duty, _pwm, speeds)) =
                    proxy.call::<_, _, (f64, u16, u8, Vec<(u8, u16, String)>)>("GetStatus", &())
                {
                    self.current_fan_data = Some(crate::fan_monitor::FanDataPoint {
                        timestamp: chrono::Local::now(),
                        temperature: temp as f32,
                        fan_speeds: speeds,
                        fan_duty: duty,
                        cpu_usage: self.fan_monitor.read_cpu_usage().unwrap_or(0.0),
                    });
                    self.last_fan_data_update = std::time::Instant::now();
                    used_daemon_status = true;
                }

                // Current duties; the full list only when the board first
                // appears, so a refresh never overwrites settings being edited.
                if let Some(fans) = fetch_channels(proxy) {
                    if self.fans.is_empty() {
                        self.fans = fans;
                    } else {
                        for fan in &mut self.fans {
                            if let Some(latest) = fans.iter().find(|f| f.number == fan.number) {
                                fan.duty = latest.duty;
                            }
                        }
                    }
                }

                self.overheat_alert = proxy
                    .call::<_, _, (bool, f64, String)>("GetOverheat", &())
                    .ok()
                    .filter(|(active, _, _)| *active)
                    .map(|(_, temp, sensor)| crate::alerts::overheat_message(temp, &sensor));
                self.fan_control_alert = proxy
                    .call::<_, _, (String, String)>("GetFanControlState", &())
                    .ok()
                    .and_then(|(state, detail)| {
                        crate::alerts::fan_control_message(&state, &detail)
                    });
            }

            if !used_daemon_status {
                // Ensure the monitor is using the currently selected curve
                if self.last_applied_curve_index != Some(self.current_curve_index) {
                    self.fan_monitor
                        .set_fan_curve(self.fan_curves[self.current_curve_index].clone());
                    self.last_applied_curve_index = Some(self.current_curve_index);
                }

                if let Ok(data) = self.fan_monitor.get_current_fan_data_sync() {
                    // Never write PWM from the GUI — sysfs requires root.
                    // Hardware control belongs exclusively to the daemon.
                    self.current_fan_data = Some(data);
                    self.last_fan_data_update = std::time::Instant::now();
                }
            }

            // Prefer the temperatures the daemon controls the fans with; read
            // them locally when no daemon (or an older one) can report them.
            let daemon_temps = self.daemon.as_ref().and_then(|proxy| {
                proxy
                    .call::<_, _, (bool, f64, String, f64)>("GetTemperatures", &())
                    .ok()
            });
            match daemon_temps {
                Some((cpu_ok, cpu, gpu_state, gpu)) => {
                    self.cpu_temp = cpu_ok.then_some(cpu as f32);
                    self.gpu_temp = Some(match gpu_state.as_str() {
                        "ok" => GpuReading::Value(gpu as f32),
                        "asleep" => GpuReading::Asleep,
                        _ => GpuReading::Unavailable,
                    });
                }
                None => {
                    self.cpu_temp = self.current_fan_data.as_ref().map(|d| d.temperature);
                    self.gpu_temp =
                        Some(self.local_gpu.get_or_insert_with(GpuTemp::discover).read());
                }
            }
            // Display only, so the GUI reads it itself rather than the daemon.
            self.gpu_usage = Some(
                self.local_gpu
                    .get_or_insert_with(GpuTemp::discover)
                    .read_usage(),
            );
        }

        // No test mode state to manage

        // Request periodic repaint for smooth updates
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Fan Curve Control");

            // CPU manufacturer information
            if let Some(sensor_info) = self.fan_monitor.cpu_temp_detector().get_sensor_info() {
                ui.horizontal(|ui| {
                    ui.label("CPU:");
                    ui.colored_label(
                        match sensor_info.manufacturer {
                            crate::cpu_temp::CpuManufacturer::Intel => egui::Color32::from_rgb(110, 170, 255),
                            crate::cpu_temp::CpuManufacturer::Amd => egui::Color32::from_rgb(255, 110, 110),
                            crate::cpu_temp::CpuManufacturer::Unknown => egui::Color32::GRAY,
                        },
                        self.cpu_model
                            .clone()
                            .unwrap_or_else(|| format!("{:?}", sensor_info.manufacturer))
                    );
                    ui.label("|");
                    ui.label(format!("Sensor: {}", sensor_info.sensor_name));
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("CPU:");
                    ui.colored_label(
                        egui::Color32::GRAY,
                        self.cpu_model.as_deref().unwrap_or("Unknown"),
                    );
                    ui.label("|");
                    ui.colored_label(egui::Color32::YELLOW, "Temperature sensor not detected");
                });
            }

            ui.horizontal(|ui| {
                ui.label("GPU:");
                if self.gpu_models.is_empty() {
                    ui.colored_label(egui::Color32::GRAY, "None detected");
                }
                for (i, gpu) in self.gpu_models.iter().enumerate() {
                    if i > 0 {
                        ui.label("|");
                    }
                    let color = match gpu.vendor {
                        crate::gpu_info::GpuVendor::Nvidia => egui::Color32::from_rgb(118, 185, 0),
                        crate::gpu_info::GpuVendor::Amd => egui::Color32::from_rgb(255, 110, 110),
                        crate::gpu_info::GpuVendor::Intel => egui::Color32::from_rgb(110, 170, 255),
                        crate::gpu_info::GpuVendor::Other => egui::Color32::GRAY,
                    };
                    ui.colored_label(color, &gpu.name);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Daemon:");
                if self.daemon.is_some() {
                    ui.colored_label(egui::Color32::GREEN, "connected (daemon controls fans)");
                } else {
                    ui.colored_label(
                        egui::Color32::RED,
                        "not running — fans will NOT be controlled",
                    );
                }
            });
            if self.daemon.is_none() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Start: sudo ./scripts/install-daemon.sh (or: sudo systemctl start fan-curve-daemon)",
                );
            }

            for (alert, color) in [
                (&self.overheat_alert, egui::Color32::RED),
                (&self.fan_control_alert, egui::Color32::YELLOW),
            ] {
                if let Some((summary, body)) = alert {
                    ui.colored_label(color, egui::RichText::new(summary).strong());
                    ui.colored_label(color, body);
                }
            }

            ui.separator();

            self.show_profile_editor(ui);

            ui.separator();

            // Save dialog
            if self.show_save_dialog {
                let mut should_close = false;
                let mut should_save = false;

                egui::Window::new("Save Profile")
                    .open(&mut self.show_save_dialog)
                    .show(ctx, |ui| {
                        ui.label("Enter profile name:");
                        ui.text_edit_singleline(&mut self.new_curve_name);

                        ui.horizontal(|ui| {
                                                if ui.button("Save").clicked() && !self.new_curve_name.is_empty() {
                        should_save = true;
                        should_close = true;
                    }
                            if ui.button("Cancel").clicked() {
                                should_close = true;
                            }
                        });
                    });

                if should_close {
                    self.show_save_dialog = false;
                    if should_save && !self.new_curve_name.is_empty() {
                        let name = self.new_curve_name.trim().to_string();
                        if FanCurve::is_builtin_name(&name) {
                            self.set_status(format!(
                                "'{}' is a built-in profile name. Choose a different name.",
                                name
                            ));
                            self.new_curve_name.clear();
                        } else if self.fan_curves.iter().any(|c| c.name() == name) {
                            self.set_status(format!(
                                "A profile named '{}' already exists. Choose a different name.",
                                name
                            ));
                            self.new_curve_name.clear();
                        } else {
                            let mut new_curve = self.fan_curves[self.current_curve_index].clone();
                            new_curve.set_name(name);
                            self.fan_curves.push(new_curve);
                            self.new_curve_name.clear();
                            self.set_status("Profile saved!".to_string());
                            self.auto_save_config();
                        }
                    } else {
                        self.new_curve_name.clear();
                    }
                }
            }

            // (Test mode removed)

            // Add point dialog
            if self.show_add_point_dialog {
                let mut should_close = false;
                let mut should_add = false;
                let mut error_message = None;

                egui::Window::new("Add Fan Curve Point")
                    .open(&mut self.show_add_point_dialog)
                    .show(ctx, |ui| {
                        ui.label("Enter temperature and fan duty for the new point:");

                        ui.horizontal(|ui| {
                            ui.label("Temperature (°C):");
                            ui.add(egui::TextEdit::singleline(&mut self.new_point_temp)
                                .desired_width(80.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Fan Duty (%):");
                            ui.add(egui::TextEdit::singleline(&mut self.new_point_duty)
                                .desired_width(80.0));
                        });

                        ui.separator();

                        ui.horizontal(|ui| {
                            if ui.button("Add Point").clicked() {
                                // Validate inputs
                                if let (Ok(temp), Ok(duty)) = (
                                    self.new_point_temp.parse::<i16>(),
                                    self.new_point_duty.parse::<u16>()
                                ) {
                                    if (0..=100).contains(&temp) && duty <= 100 {
                                        should_add = true;
                                        should_close = true;
                                    } else {
                                        error_message = Some("Invalid values: Temperature must be 0-100°C, Duty must be 0-100%".to_string());
                                    }
                                } else {
                                    error_message = Some("Invalid input: Please enter valid numbers".to_string());
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                should_close = true;
                            }
                        });
                    });

                if should_close {
                    self.show_add_point_dialog = false;
                    if should_add {
                        if let (Ok(temp), Ok(duty)) = (
                            self.new_point_temp.parse::<i16>(),
                            self.new_point_duty.parse::<u16>()
                        ) {
                            // Dialog input is percent; store as ten-thousandths (0-10000)
                            self.add_target_point(temp, duty * 100);
                            self.set_status(format!("Added point: {}°C -> {}%", temp, duty));
                            self.auto_save_config();
                        }
                        self.new_point_temp.clear();
                        self.new_point_duty.clear();
                    } else {
                        self.new_point_temp.clear();
                        self.new_point_duty.clear();
                    }
                }

                if let Some(error) = error_message {
                    self.set_status(error);
                }
            }

            // Edit point dialog
            if self.show_edit_point_dialog {
                let mut should_close = false;
                let mut should_edit = false;
                let mut error_message = None;

                egui::Window::new("Edit Fan Curve Point")
                    .open(&mut self.show_edit_point_dialog)
                    .show(ctx, |ui| {
                        ui.label("Edit temperature and fan duty for the selected point:");

                        ui.horizontal(|ui| {
                            ui.label("Temperature (°C):");
                            ui.add(egui::TextEdit::singleline(&mut self.edit_point_temp)
                                .desired_width(80.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Fan Duty (%):");
                            ui.add(egui::TextEdit::singleline(&mut self.edit_point_duty)
                                .desired_width(80.0));
                        });

                        ui.separator();

                        ui.horizontal(|ui| {
                            if ui.button("Update Point").clicked() {
                                // Validate inputs
                                if let (Ok(temp), Ok(duty)) = (
                                    self.edit_point_temp.parse::<i16>(),
                                    self.edit_point_duty.parse::<u16>()
                                ) {
                                    if (0..=100).contains(&temp) && duty <= 100 {
                                        should_edit = true;
                                        should_close = true;
                                    } else {
                                        error_message = Some("Invalid values: Temperature must be 0-100°C, Duty must be 0-100%".to_string());
                                    }
                                } else {
                                    error_message = Some("Invalid input: Please enter valid numbers".to_string());
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                should_close = true;
                            }
                        });
                    });

                if should_close {
                    self.show_edit_point_dialog = false;
                    if should_edit {
                        if let (Ok(temp), Ok(duty)) = (
                            self.edit_point_temp.parse::<i16>(),
                            self.edit_point_duty.parse::<u16>()
                        ) {
                            if let Some(index) = self.edit_point_index {
                                // Remove the old point and add the new one
                                // Dialog input is percent; store as ten-thousandths (0-10000)
                                if self.remove_target_point(index).is_some() {
                                    self.add_target_point(temp, duty * 100);
                                    self.set_status(format!("Updated point {}: {}°C -> {}%", index + 1, temp, duty));
                                    self.auto_save_config();
                                }
                            }
                        }
                        self.edit_point_temp.clear();
                        self.edit_point_duty.clear();
                        self.edit_point_index = None;
                    } else {
                        self.edit_point_temp.clear();
                        self.edit_point_duty.clear();
                        self.edit_point_index = None;
                    }
                }

                if let Some(error) = error_message {
                    self.set_status(error);
                }
            }

            // Status message
            if let Some(status) = &self.status_message {
                ui.label(status);
            }
        });

        // Bottom panel for live fan data (always visible)
        egui::TopBottomPanel::bottom("live_fan_data")
            .resizable(true)
            .min_height(120.0)
            .show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    ui.available_size(),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.heading("Live Fan Data");
                        });

                        ui.separator();

                        // Live data display
                        if let Some(ref data) = self.current_fan_data {
                            ui.horizontal(|ui| {
                                // Temperature and Fan Speed
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("CPU Temp:");
                                        match self.cpu_temp {
                                            Some(t) => ui.colored_label(
                                                temp_color(t),
                                                format!("{:.1}°C", t),
                                            ),
                                            None => {
                                                ui.colored_label(egui::Color32::GRAY, "Unavailable")
                                            }
                                        };
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("GPU Temp:");
                                        match self.gpu_temp {
                                            Some(GpuReading::Value(t)) => ui.colored_label(
                                                temp_color(t),
                                                format!("{:.1}°C", t),
                                            ),
                                            Some(GpuReading::Asleep) => ui.colored_label(
                                                egui::Color32::GRAY,
                                                "Off (powered down)",
                                            ),
                                            _ => {
                                                ui.colored_label(egui::Color32::GRAY, "Unavailable")
                                            }
                                        };
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Fan Speeds:");
                                        if data.fan_speeds.is_empty() {
                                            ui.colored_label(
                                                egui::Color32::GRAY,
                                                "No fans detected",
                                            );
                                        } else {
                                            for (i, (_num, speed, label)) in
                                                data.fan_speeds.iter().enumerate()
                                            {
                                                if i > 0 {
                                                    ui.label(" | ");
                                                }
                                                ui.colored_label(
                                                    if *speed > 2500 {
                                                        egui::Color32::RED
                                                    } else if *speed > 1500 {
                                                        egui::Color32::YELLOW
                                                    } else {
                                                        egui::Color32::GREEN
                                                    },
                                                    format!("{}: {} RPM", label, speed),
                                                );
                                            }
                                        }
                                    });
                                });

                                ui.add_space(20.0);

                                // Fan Duty and CPU Usage
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Fan Duty:");
                                        // Each fan's duty when the daemon reports them
                                        let duties: Vec<(Option<&str>, u16)> =
                                            if self.fans.is_empty() {
                                                vec![(None, data.fan_duty)]
                                            } else {
                                                self.fans
                                                    .iter()
                                                    .map(|f| (Some(f.label.as_str()), f.duty))
                                                    .collect()
                                            };
                                        for (i, (label, duty)) in duties.into_iter().enumerate() {
                                            if i > 0 {
                                                ui.label(" | ");
                                            }
                                            let percent = duty / 100; // ten-thousandths → %
                                            ui.colored_label(
                                                if duty > 8000 {
                                                    egui::Color32::RED
                                                } else if duty > 5000 {
                                                    egui::Color32::YELLOW
                                                } else {
                                                    egui::Color32::GREEN
                                                },
                                                match label {
                                                    Some(label) => {
                                                        format!("{}: {}%", label, percent)
                                                    }
                                                    None => format!("{}%", percent),
                                                },
                                            );
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("CPU Usage:");
                                        ui.colored_label(
                                            if data.cpu_usage > 80.0 {
                                                egui::Color32::RED
                                            } else if data.cpu_usage > 50.0 {
                                                egui::Color32::YELLOW
                                            } else {
                                                egui::Color32::GREEN
                                            },
                                            format!("{:.1}%", data.cpu_usage),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("GPU Usage:");
                                        match self.gpu_usage {
                                            Some(GpuReading::Value(u)) => ui.colored_label(
                                                if u > 80.0 {
                                                    egui::Color32::RED
                                                } else if u > 50.0 {
                                                    egui::Color32::YELLOW
                                                } else {
                                                    egui::Color32::GREEN
                                                },
                                                format!("{:.0}%", u),
                                            ),
                                            Some(GpuReading::Asleep) => ui.colored_label(
                                                egui::Color32::GRAY,
                                                "Off (powered down)",
                                            ),
                                            _ => {
                                                ui.colored_label(egui::Color32::GRAY, "Unavailable")
                                            }
                                        };
                                    });
                                });

                                ui.add_space(20.0);

                                // Timestamp
                                ui.vertical(|ui| {
                                    ui.label("Last Update:");
                                    ui.label(data.timestamp.format("%H:%M:%S").to_string());
                                });
                            });

                            // (No progress bar; test mode removed)
                        } else {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Collecting fan data...");
                            });
                        }
                    },
                );
            });
    }
}
