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

impl FanCurveApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Try to load existing config, fallback to default
        let config_path = FanCurveConfig::get_config_path();
        let (fan_curves, default_curve_index) = if config_path.exists() {
            match FanCurveConfig::load_from_file(&config_path) {
                Ok(config) => (config.curves, config.default_curve_index),
                Err(_) => {
                    let default_config = FanCurveConfig::new();
                    (default_config.curves, default_config.default_curve_index)
                }
            }
        } else {
            let default_config = FanCurveConfig::new();
            (default_config.curves, default_config.default_curve_index)
        };

        Self {
            fan_curves,
            current_curve_index: default_curve_index.unwrap_or(0),
            default_curve_index,
            status_message: None,
            new_curve_name: String::new(),
            show_save_dialog: false,
            fan_monitor: FanMonitor::new(),
            current_fan_data: None,
            last_fan_data_update: std::time::Instant::now(),
            show_add_point_dialog: false,
            new_point_temp: String::new(),
            new_point_duty: String::new(),
            show_edit_point_dialog: false,
            edit_point_index: None,
            edit_point_temp: String::new(),
            edit_point_duty: String::new(),
        }
    }

    fn save_config(&self) -> Result<()> {
        let config_path = FanCurveConfig::get_config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let config = FanCurveConfig {
            curves: self.fan_curves.clone(),
            default_curve_index: self.default_curve_index,
        };

        config.save_to_file(&config_path)?;
        Ok(())
    }

    fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
    }
}

impl eframe::App for FanCurveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Always update live fan data every 1s
        if self.last_fan_data_update.elapsed() >= std::time::Duration::from_secs(1) {
            if let Ok(data) = self.fan_monitor.get_current_fan_data_sync() {
                println!(
                    "🔄 GUI: Updated fan data - Temp: {:.1}°C, Fan: {} RPM, Duty: {}%",
                    data.temperature, data.fan_speed, data.fan_duty
                );
                self.current_fan_data = Some(data);
                self.last_fan_data_update = std::time::Instant::now();
            }
        }

        // No test mode state to manage

        // Request periodic repaint for smooth updates
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Fan Curve Control");

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
                    ui.label(format!("{}°C -> {}%", point.temp, point.duty));

                    ui.add_space(10.0);

                    if ui.button("Edit").clicked() {
                        self.show_edit_point_dialog = true;
                        self.edit_point_index = Some(i);
                        self.edit_point_temp = point.temp.to_string();
                        self.edit_point_duty = point.duty.to_string();
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
                        removed_point.duty
                    ));
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

            // Apply button
            if ui.button("Apply Fan Curve").clicked() {
                match self.save_config() {
                    Ok(_) => self.set_status("Fan curve applied and saved!".to_string()),
                    Err(e) => self.set_status(format!("Failed to save: {}", e)),
                }
            }

            // Set as default button
            if ui.button("Set as Default").clicked() {
                self.default_curve_index = Some(self.current_curve_index);
                match self.save_config() {
                    Ok(_) => self.set_status("Set as default and saved!".to_string()),
                    Err(e) => self.set_status(format!("Failed to save: {}", e)),
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
                            self.fan_curves[self.current_curve_index].add_point(temp, duty);
                            self.set_status(format!("Added point: {}°C -> {}%", temp, duty));
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
                                    if let Some(_old_point) = self.fan_curves[self.current_curve_index].remove_point(index) {
                                        self.fan_curves[self.current_curve_index].add_point(temp, duty);
                                        self.set_status(format!("Updated point {}: {}°C -> {}%", index + 1, temp, duty));
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
                                        ui.label("🌀 Fan Speed:");
                                        ui.colored_label(
                                            if data.fan_speed > 2500 {
                                                egui::Color32::RED
                                            } else if data.fan_speed > 1500 {
                                                egui::Color32::YELLOW
                                            } else {
                                                egui::Color32::GREEN
                                            },
                                            format!("{} RPM", data.fan_speed),
                                        );
                                    });
                                });

                                ui.add_space(20.0);

                                // Fan Duty and CPU Usage
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("⚡ Fan Duty:");
                                        ui.colored_label(
                                            if data.fan_duty > 80 {
                                                egui::Color32::RED
                                            } else if data.fan_duty > 50 {
                                                egui::Color32::YELLOW
                                            } else {
                                                egui::Color32::GREEN
                                            },
                                            format!("{}%", data.fan_duty),
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
