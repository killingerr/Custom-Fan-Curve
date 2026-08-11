use crate::errors::Result;
use crate::fan::{FanCurve, FanCurveConfig};
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
    let current = default.unwrap_or(0).min(config.curves.len().saturating_sub(1));
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
            eprintln!("Warning: Failed to initialize CPU temperature detection: {}", e);
            eprintln!("Falling back to simulation mode");
        }

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

        let config = FanCurveConfig {
            curves: self.fan_curves.clone(),
            default_curve_index: self.default_curve_index,
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
        
        println!("Configuration saved successfully to: {}", config_path.display());
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
            .call::<_, _, ()>(
                "SetConfig",
                &(curves, default_index, current_name.as_str()),
            )
            .map_err(|e| e.to_string())?;

        Ok(true)
    }

    /// Auto-save configuration with error handling
    fn auto_save_config(&mut self) {
        if let Err(e) = self.save_config() {
            eprintln!("Auto-save failed: {}", e);
            self.set_status(format!("Auto-save failed: {}", e));
            return;
        }

        match self.push_config_to_daemon() {
            Ok(true) => self.set_status("Configuration saved and pushed to daemon".to_string()),
            Ok(false) => self.set_status("Configuration saved".to_string()),
            Err(e) => self.set_status(format!("Saved locally, but daemon push failed: {}", e)),
        }
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
                        cpu_usage: 0.0,
                    });
                    self.last_fan_data_update = std::time::Instant::now();
                    used_daemon_status = true;
                }
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
                    ui.label("🖥️ CPU:");
                    ui.colored_label(
                        match sensor_info.manufacturer {
                            crate::cpu_temp::CpuManufacturer::Intel => egui::Color32::BLUE,
                            crate::cpu_temp::CpuManufacturer::Amd => egui::Color32::RED,
                            crate::cpu_temp::CpuManufacturer::Unknown => egui::Color32::GRAY,
                        },
                        format!("{:?}", sensor_info.manufacturer)
                    );
                    ui.label("|");
                    ui.label(format!("Sensor: {}", sensor_info.sensor_name));
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("🖥️ CPU:");
                    ui.colored_label(egui::Color32::GRAY, "Unknown");
                    ui.label("|");
                    ui.colored_label(egui::Color32::YELLOW, "Temperature sensor not detected");
                });
            }

            ui.horizontal(|ui| {
                ui.label("🔌 Daemon:");
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
                    "Start: sudo ./scripts/install-daemon.sh   (or: sudo systemctl start fan-curve-daemon)",
                );
            }

            ui.separator();

            // Current fan profile display
            let current_profile = self.fan_curves[self.current_curve_index].name();
            ui.label(format!("Current Profile: {}", current_profile));

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

            // Display fan curve points
            ui.separator();
            ui.label("Fan Curve Points:");

            let mut points_to_remove = Vec::new();

            // First pass: display points and collect indices to remove
            for (i, point) in self.fan_curves[self.current_curve_index].points().iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Point {}: ", i + 1));
                    ui.label(format!("{}°C -> {}%", point.temp, point.duty_percent()));

                    ui.add_space(10.0);

                    if ui.button("Edit").clicked() {
                        self.show_edit_point_dialog = true;
                        self.edit_point_index = Some(i);
                        self.edit_point_temp = point.temp.to_string();
                        self.edit_point_duty = point.duty_percent().to_string();
                    }

                    ui.add_space(5.0);

                    if ui.button("Remove").clicked() {
                        points_to_remove.push(i);
                    }
                });
            }

            // Second pass: remove points in reverse order to maintain indices
            for &index in points_to_remove.iter().rev() {
                if let Some(removed_point) = self.fan_curves[self.current_curve_index].remove_point(index) {
                    self.set_status(format!("Removed point {}: {}°C -> {}%",
                        index + 1,
                        removed_point.temp,
                        removed_point.duty_percent()
                    ));
                    // Auto-save after removing point
                    self.auto_save_config();
                }
            }

            ui.separator();

            // Add point button
            if ui.button("Add Point").clicked() {
                self.show_add_point_dialog = true;
                self.new_point_temp = "50".to_string();
                self.new_point_duty = "50".to_string();
            }

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
                        let mut new_curve = self.fan_curves[self.current_curve_index].clone();
                        new_curve.set_name(self.new_curve_name.clone());
                        self.fan_curves.push(new_curve);
                        self.new_curve_name.clear();
                        self.set_status("Profile saved!".to_string());
                        // Auto-save after creating new profile
                        self.auto_save_config();
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
                            self.fan_curves[self.current_curve_index].add_point(temp, duty * 100);
                            self.set_status(format!("Added point: {}°C -> {}%", temp, duty));
                            // Auto-save after adding point
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
                                if index < self.fan_curves[self.current_curve_index].points().len() {
                                    // Remove the old point and add the new one
                                    // Dialog input is percent; store as ten-thousandths (0-10000)
                                    if let Some(_old_point) = self.fan_curves[self.current_curve_index].remove_point(index) {
                                        self.fan_curves[self.current_curve_index].add_point(temp, duty * 100);
                                        self.set_status(format!("Updated point {}: {}°C -> {}%", index + 1, temp, duty));
                                        // Auto-save after editing point
                                        self.auto_save_config();
                                    }
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
                            ui.heading("🌡️ Live Fan Data");
                        });

                        ui.separator();

                        // Live data display
                        if let Some(ref data) = self.current_fan_data {
                            ui.horizontal(|ui| {
                                // Temperature and Fan Speed
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("🌡️ Temperature:");
                                        ui.colored_label(
                                            if data.temperature > 70.0 {
                                                egui::Color32::RED
                                            } else if data.temperature > 50.0 {
                                                egui::Color32::YELLOW
                                            } else {
                                                egui::Color32::GREEN
                                            },
                                            format!("{:.1}°C", data.temperature),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("🌀 Fan Speeds:");
                                        if data.fan_speeds.is_empty() {
                                            ui.colored_label(egui::Color32::GRAY, "No fans detected");
                                        } else {
                                            for (i, (_num, speed, label)) in data.fan_speeds.iter().enumerate() {
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
                                        ui.label("⚡ Fan Duty:");
                                        ui.colored_label(
                                            if data.fan_duty > 8000 {
                                                egui::Color32::RED
                                            } else if data.fan_duty > 5000 {
                                                egui::Color32::YELLOW
                                            } else {
                                                egui::Color32::GREEN
                                            },
                                            format!("{}%", data.fan_duty / 100), // ten-thousandths → %
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("💻 CPU Usage:");
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
                                });

                                ui.add_space(20.0);

                                // Timestamp
                                ui.vertical(|ui| {
                                    ui.label("⏰ Last Update:");
                                    ui.label(data.timestamp.format("%H:%M:%S").to_string());
                                });
                            });

                            // (No progress bar; test mode removed)
                        } else {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("🔄 Collecting fan data...");
                            });
                        }
                    },
                );
            });
    }
}
