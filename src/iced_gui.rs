use crate::errors::Result;
use crate::fan::{FanCurve, FanCurveConfig};
use crate::fan_monitor::FanMonitor;
use iced::{
    widget::{button, container, Column, Row, Text, text_input, pick_list},
    Application, Command, Element, Length, Settings, Theme,
    alignment::Alignment,
};

#[derive(Debug, Clone)]
pub enum Message {
    // Fan curve selection
    CurveSelected(FanCurve),
    
    // Fan curve editing
    AddPoint,
    RemovePoint(usize),
    EditPoint(usize),
    EditTempChanged(String),
    EditDutyChanged(String),
    SaveEdit,
    CancelEdit,
    
    // Actions
    ApplyFanCurve,
    SetFanDuty(u8),
    SaveAsNewProfile,
    SetAsDefault,
    
    // Profile management
    NewProfileNameChanged(String),
    SaveNewProfile,
    CancelSaveProfile,
    
    // Data updates
    DataUpdated(std::result::Result<crate::fan_monitor::FanDataPoint, String>),
    Tick, // For automatic updates
}

pub struct FanCurveApp {
    // Fan curves and selection
    fan_curves: Vec<FanCurve>,
    current_curve_index: usize,
    default_curve_index: Option<usize>,
    
    // UI state
    status_message: Option<String>,
    show_save_dialog: bool,
    editing_point: Option<usize>,
    edit_temp_input: String,
    edit_duty_input: String,
    
    // Profile saving
    new_profile_name: String,
    
    // Fan monitoring
    fan_monitor: FanMonitor,
    current_data: Option<crate::fan_monitor::FanDataPoint>,
    data_error: Option<String>,
}

impl FanCurveApp {
    pub fn new() -> Self {
        // Load existing config or use defaults
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

        // Initialize fan monitor
        let fan_monitor = FanMonitor::new();
        // Note: We'll initialize the System76 Power client later in the Application::new method
        
        Self {
            fan_curves,
            current_curve_index: default_curve_index.unwrap_or(0),
            default_curve_index,
            status_message: None,
            show_save_dialog: false,
            editing_point: None,
            edit_temp_input: String::new(),
            edit_duty_input: String::new(),
            new_profile_name: String::new(),
            fan_monitor,
            current_data: None,
            data_error: None,
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

    fn clear_status(&mut self) {
        self.status_message = None;
    }

}

impl Application for FanCurveApp {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        let app = Self::new();
        
        // Start with a Tick message to begin automatic updates
        let init_command = Command::perform(
            async { Message::Tick },
            |_| Message::Tick,
        );
        
        (app, init_command)
    }

    fn title(&self) -> String {
        "Fan Curve Control".to_string()
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        log::info!("GUI: Received message: {:?}", message);
        match message {
            Message::CurveSelected(curve) => {
                // Find the index of the selected curve
                if let Some(index) = self.fan_curves.iter().position(|c| c.name() == curve.name()) {
                    self.current_curve_index = index;
                    self.clear_status();
                }
                Command::none()
            }

            Message::AddPoint => {
                log::info!("GUI: AddPoint button clicked - this proves GUI messages work!");
                self.set_status("Add Point clicked (not implemented yet)".to_string());
                Command::none()
            }

            Message::RemovePoint(index) => {
                if let Some(removed_point) = self.fan_curves[self.current_curve_index].remove_point(index) {
                    self.set_status(format!("Removed point {}: {}°C -> {}%",
                        index + 1,
                        removed_point.temp,
                        removed_point.duty
                    ));
                    
                    // Save the updated configuration
                    if let Err(e) = self.save_config() {
                        self.set_status(format!("Point removed but failed to save: {}", e));
                    }
                }
                Command::none()
            }

            Message::EditPoint(index) => {
                // Start editing the point
                if let Some(point) = self.fan_curves[self.current_curve_index].get_point(index) {
                    self.editing_point = Some(index);
                    self.edit_temp_input = point.temp.to_string();
                    // Convert ten-thousandths to percentage for user input
                    self.edit_duty_input = (point.duty as f32 / 100.0).to_string();
                    self.set_status(format!("Editing point {}: {}°C -> {:.1}%", index + 1, point.temp, point.duty as f32 / 100.0));
                } else {
                    self.set_status(format!("Point {} not found", index + 1));
                }
                Command::none()
            }

            Message::EditTempChanged(value) => {
                self.edit_temp_input = value;
                Command::none()
            }

            Message::EditDutyChanged(value) => {
                self.edit_duty_input = value;
                Command::none()
            }

            Message::SaveEdit => {
                if let Some(point_index) = self.editing_point {
                    // Parse the input values
                    let temp: std::result::Result<f32, _> = self.edit_temp_input.parse();
                    let duty_percent: std::result::Result<f32, _> = self.edit_duty_input.parse();
                    
                    match (temp, duty_percent) {
                        (Ok(temp_val), Ok(duty_percent_val)) => {
                            // Validate ranges
                            if temp_val < 0.0 || temp_val > 100.0 {
                                self.set_status("Temperature must be between 0 and 100°C".to_string());
                            } else if duty_percent_val < 0.0 || duty_percent_val > 100.0 {
                                self.set_status("Duty must be between 0 and 100%".to_string());
                            } else {
                                // Convert percentage to ten-thousandths for storage
                                let duty_ten_thousandths = (duty_percent_val * 100.0) as u16;
                                
                                // Update the point
                                if let Some(point) = self.fan_curves[self.current_curve_index].get_point_mut(point_index) {
                                    point.temp = temp_val as i16; // Convert f32 to i16
                                    point.duty = duty_ten_thousandths;
                                    self.set_status(format!("Point {} updated: {}°C -> {:.1}%", 
                                        point_index + 1, temp_val, duty_percent_val));
                                    
                                    // Save the updated configuration
                                    if let Err(e) = self.save_config() {
                                        self.set_status(format!("Point updated but failed to save: {}", e));
                                    }
                                }
                                
                                // Clear editing state
                                self.editing_point = None;
                                self.edit_temp_input.clear();
                                self.edit_duty_input.clear();
                            }
                        }
                        _ => {
                            self.set_status("Invalid input: temperature and duty must be numbers".to_string());
                        }
                    }
                }
                Command::none()
            }

            Message::CancelEdit => {
                // Clear editing state
                self.editing_point = None;
                self.edit_temp_input.clear();
                self.edit_duty_input.clear();
                self.set_status("Edit cancelled".to_string());
                Command::none()
            }

                    Message::ApplyFanCurve => {
                        log::info!("=== GUI: ApplyFanCurve button clicked ===");
                        
                        // Extract ALL data first, then do everything else
                        let curve_index = self.current_curve_index;
                        let (curve_name, current_curve, temperature) = if let Some(ref data) = self.current_data {
                            log::info!("GUI: Using current temperature data: {:.1}°C", data.temperature);
                            (self.fan_curves[curve_index].name().clone(), 
                             self.fan_curves[curve_index].clone(), 
                             data.temperature)
                        } else {
                            log::error!("GUI: No temperature data available - cannot apply fan curve");
                            self.set_status("No temperature data available - cannot apply fan curve".to_string());
                            return Command::none();
                        };
                        
                        log::info!("GUI: About to apply fan curve '{}' with {} points", curve_name, current_curve.points().len());
                        
                        // Now we can safely call methods that require &mut self
                        let result = self.fan_monitor.apply_fan_curve_from_gui(&current_curve, temperature);
                        
                        // Build status messages separately to avoid borrow issues
                        let status_msg = if result.is_ok() {
                            format!("Fan curve '{}' applied successfully! Temperature: {:.1}°C", curve_name, temperature)
                        } else {
                            format!("Failed to apply fan curve '{}': {}", curve_name, result.as_ref().unwrap_err())
                        };
                        
                        // Now set status (mutable borrow)
                        self.set_status(status_msg);
                        
                        // Log after status is set
                        if result.is_ok() {
                            log::info!("GUI: Fan curve applied successfully via direct PWM: {:.1}°C", temperature);
                        } else {
                            log::error!("GUI: Failed to apply fan curve: {}", result.unwrap_err());
                        }
                        
                        log::info!("=== GUI: ApplyFanCurve completed ===");
                        Command::none()
                    }

            Message::SetFanDuty(duty_percent) => {
                // Convert percentage (0-100) to PWM value (0-255)
                let pwm_value = if duty_percent == 0 {
                    0 // Auto mode
                } else {
                    ((duty_percent as f32 / 100.0) * 255.0) as u8
                };
                
                // Set fan duty directly via D-Bus
                let result = self.fan_monitor.set_fan_duty_from_gui(pwm_value);
                
                // Build status message
                let status_msg = if result.is_ok() {
                    if duty_percent == 0 {
                        "Fan duty set to Auto mode".to_string()
                    } else {
                        format!("Fan duty set to {}% (PWM: {})", duty_percent, pwm_value)
                    }
                } else {
                    format!("Failed to set fan duty to {}%: {}", duty_percent, result.as_ref().unwrap_err())
                };
                
                // Set status
                self.set_status(status_msg);
                
                // Log result
                if result.is_ok() {
                    if duty_percent == 0 {
                        log::info!("Fan duty set to Auto mode via D-Bus");
                    } else {
                        log::info!("Fan duty set to {}% (PWM: {}) via D-Bus", duty_percent, pwm_value);
                    }
                } else {
                    log::error!("Failed to set fan duty to {}%: {}", duty_percent, result.unwrap_err());
                }
                
                Command::none()
            }

            Message::SaveAsNewProfile => {
                self.show_save_dialog = true;
                self.new_profile_name = String::new();
                Command::none()
            }

            Message::SetAsDefault => {
                self.default_curve_index = Some(self.current_curve_index);
                if let Err(e) = self.save_config() {
                    self.set_status(format!("Failed to save: {}", e));
                } else {
                    self.set_status("Set as default and saved!".to_string());
                }
                Command::none()
            }

            Message::NewProfileNameChanged(name) => {
                self.new_profile_name = name;
                Command::none()
            }

            Message::SaveNewProfile => {
                if !self.new_profile_name.trim().is_empty() {
                    let mut new_curve = self.fan_curves[self.current_curve_index].clone();
                    new_curve.set_name(self.new_profile_name.trim().to_string());
                    self.fan_curves.push(new_curve);
                    self.set_status("Profile saved!".to_string());
                    self.show_save_dialog = false;
                } else {
                    self.set_status("Profile name cannot be empty".to_string());
                }
                Command::none()
            }

                    Message::CancelSaveProfile => {
                        self.show_save_dialog = false;
                        self.new_profile_name.clear();
                        Command::none()
                    }

                    Message::DataUpdated(result) => {
                        match result {
                            Ok(data) => {
                                self.current_data = Some(data);
                                self.data_error = None;
                                log::debug!("Updated fan data: {:.1}°C, duty: {}%", 
                                    self.current_data.as_ref().unwrap().temperature,
                                    self.current_data.as_ref().unwrap().fan_duty
                                );
                            }
                            Err(e) => {
                                self.data_error = Some(e);
                                self.current_data = None;
                                log::warn!("Failed to get fan data: {}", self.data_error.as_ref().unwrap());
                            }
                        }
                        Command::none()
                    }

                    Message::Tick => {
                        // Get data using direct file reading (no D-Bus needed for display)
                        match self.fan_monitor.get_current_fan_data_direct() {
                            Ok(data) => {
                                self.current_data = Some(data);
                                self.data_error = None;
                                log::debug!("Auto refresh - Updated fan data: {:.1}°C, duty: {:.1}%", 
                                    self.current_data.as_ref().unwrap().temperature,
                                    self.current_data.as_ref().unwrap().fan_duty as f32 / 100.0
                                );
                            }
                            Err(e) => {
                                self.data_error = Some(e.to_string());
                                self.current_data = None;
                                log::warn!("Failed to get fan data: {}", e);
                            }
                        }
                        
                        // Schedule next update using std::thread::sleep
                        return Command::perform(
                            async {
                                std::thread::sleep(std::time::Duration::from_millis(500));
                                Message::Tick
                            },
                            |msg| msg,
                        );
                    }
                }
    }

    fn view(&self) -> Element<Message> {
        let mut content = Column::new()
            .spacing(25)
            .padding(30)
            .align_items(Alignment::Center);

        // Title
        content = content.push(
            Text::new("Fan Curve Control")
                .size(28)
        );

        // Fan curve selection card
        let curve_selection = Row::new()
            .spacing(15)
            .align_items(Alignment::Center)
            .push(
                Text::new("Select Profile:")
                    .size(14)
            )
            .push(
                pick_list(
                    self.fan_curves.as_slice(),
                    Some(self.fan_curves[self.current_curve_index].clone()),
                    Message::CurveSelected,
                )
                .width(200)
            )
            .push(
                button("Set Default")
                    .padding([8, 16])
                    .on_press(Message::SetAsDefault)
            );

        let curve_card = Column::new()
            .spacing(15)
            .push(
                Text::new("📋 Fan Curve Selection")
                    .size(18)
            )
            .push(curve_selection);

        content = content.push(
            container(curve_card)
                .padding(20)
        );

        // Fan curve points card
        let mut points_content = Column::new().spacing(10);
        
        for (i, point) in self.fan_curves[self.current_curve_index].points().iter().enumerate() {
            let point_row = Row::new()
                .spacing(15)
                .align_items(Alignment::Center)
                .push(
                    Text::new(format!("Point {}: {}°C → {:.1}%", i + 1, point.temp, point.duty as f32 / 100.0))
                        .size(14)
                )
                .push(
                    button("Edit")
                        .padding([8, 16])
                        .on_press(Message::EditPoint(i))
                )
                .push(
                    button("Remove")
                        .padding([8, 16])
                        .style(iced::theme::Button::Destructive)
                        .on_press(Message::RemovePoint(i))
                );
            
            points_content = points_content.push(point_row);
        }

        // Add editing interface if a point is being edited
        if let Some(point_index) = self.editing_point {
            let edit_row = Row::new()
                .spacing(10)
                .push(
                    Text::new(format!("Editing Point {}:", point_index + 1))
                        .size(16)
                )
                .push(
                    Text::new("Temp (°C):")
                        .size(14)
                )
                .push(
                    text_input("Temperature", &self.edit_temp_input)
                        .on_input(Message::EditTempChanged)
                        .width(80)
                )
                .push(
                    Text::new("Duty (%):")
                        .size(14)
                )
                .push(
                    text_input("Duty", &self.edit_duty_input)
                        .on_input(Message::EditDutyChanged)
                        .width(80)
                )
                .push(
                    button("Save")
                        .padding([6, 12])
                        .on_press(Message::SaveEdit)
                )
                .push(
                    button("Cancel")
                        .padding([6, 12])
                        .style(iced::theme::Button::Destructive)
                        .on_press(Message::CancelEdit)
                );
            
            points_content = points_content.push(edit_row);
        }

        // Action buttons for points
        let action_buttons = Row::new()
            .spacing(10)
            .push(
                button("Add Point")
                    .padding([8, 16])
                    .on_press(Message::AddPoint)
            )
            .push(
                button("Apply Fan Curve")
                    .padding([8, 16])
                    .on_press(Message::ApplyFanCurve)
            )
            .push(
                button("Save as New Profile")
                    .padding([8, 16])
                    .on_press(Message::SaveAsNewProfile)
            );

        // Fan Duty Control Section
        let fan_duty_controls = Row::new()
            .spacing(10)
            .push(
                Text::new("Fan Duty:")
                    .size(16)
            )
            .push(
                button("25%")
                    .padding([6, 12])
                    .on_press(Message::SetFanDuty(25)) // 25% duty
            )
            .push(
                button("50%")
                    .padding([6, 12])
                    .on_press(Message::SetFanDuty(50)) // 50% duty
            )
            .push(
                button("75%")
                    .padding([6, 12])
                    .on_press(Message::SetFanDuty(75)) // 75% duty
            )
            .push(
                button("100%")
                    .padding([6, 12])
                    .on_press(Message::SetFanDuty(100)) // 100% duty
            )
            .push(
                button("Auto")
                    .padding([6, 12])
                    .on_press(Message::SetFanDuty(0)) // 0 = auto mode
            );

        let points_card_content = Column::new()
            .spacing(15)
            .push(
                Text::new("⚙️ Fan Curve Points")
                    .size(18)
            )
            .push(points_content)
            .push(action_buttons);

        content = content.push(
            container(points_card_content)
                .padding(20)
        );

        // Live fan data card
        let live_data = Column::new()
            .spacing(8)
            .push(
                Text::new("📊 Live Fan Data")
                    .size(18)
            )
            .push(
                if let Some(ref data) = self.current_data {
                    Column::new()
                        .spacing(8)
                        .push(
                            Text::new(format!("🌡️ CPU Temperature: {:.1}°C", data.temperature))
                                .size(16)
                        )
                        .push(
                            Text::new(format!("🌀 Fan Duty: {:.1}%", data.fan_duty as f32 / 100.0))
                                .size(16)
                        )
                        .push(
                            Text::new(format!("⚡ CPU Usage: {:.1}%", data.cpu_usage))
                                .size(16)
                        )
                        .push(
                            Text::new(format!("💨 Fan RPMs: {}", 
                                if data.cpu_fan_speeds.is_empty() {
                                    "No fans detected".to_string()
                                } else {
                                    data.cpu_fan_speeds.iter()
                                        .map(|(_, rpm, _)| rpm.to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                }))
                                .size(16)
                        )
                        .push(
                            Text::new(format!("🔄 CPU Fans: {} detected", data.cpu_fan_speeds.len()))
                                .size(14)
                        )
                        .push(
                            Text::new(format!("💨 Intake Fans: {} detected", data.intake_fan_speeds.len()))
                                .size(14)
                        )
                        .push(
                            Text::new(format!("📊 GPU Fans: {} detected", data.gpu_fan_speeds.len()))
                                .size(14)
                        )
                        .push(
                            Text::new(format!("🕐 Last Update: {}", data.timestamp.format("%H:%M:%S")))
                                .size(12)
                        )
                        .push(
                            Text::new(format!("💻 CPU: {}", data.cpu_model))
                                .size(14)
                        )
                } else if let Some(ref error) = self.data_error {
                    Column::new()
                        .spacing(5)
                        .push(
                            Text::new("❌ Sensor Data Unavailable")
                                .size(16)
                        )
                        .push(
                            Text::new("Could not read sensor data from system files.")
                                .size(14)
                        )
                        .push(
                            Text::new("This may be due to:")
                                .size(14)
                        )
                        .push(
                            Text::new("  • Insufficient permissions")
                                .size(12)
                        )
                        .push(
                            Text::new("  • Missing sensor drivers")
                                .size(12)
                        )
                        .push(
                            Text::new("  • Hardware not detected")
                                .size(12)
                        )
                        .push(
                            Text::new(format!("Error: {}", error))
                                .size(12)
                        )
                } else {
                    Column::new()
                        .spacing(5)
                        .push(
                            Text::new("⏳ Loading data...")
                                .size(14)
                        )
                }
            );

        content = content.push(
            container(live_data)
                .padding(20)
        );

        // Fan Duty Control Card
        content = content.push(
            container(fan_duty_controls)
                .padding(20)
        );

        // Status message card
        if let Some(ref status) = self.status_message {
            let status_content = Column::new()
                .spacing(15)
                .push(
                    Text::new("💬 Status")
                        .size(18)
                )
                .push(
                    Text::new(status)
                        .size(14)
                );

            content = content.push(
                container(status_content)
                    .padding(20)
            );
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        // Use a simple subscription that triggers immediately
        iced::Subscription::none()
    }
}

pub fn run_iced_gui() -> Result<()> {
    FanCurveApp::run(Settings {
        window: iced::window::Settings {
            size: iced::Size::new(800.0, 600.0),
            ..Default::default()
        },
        ..Settings::with_flags(())
    })
        .map_err(|e| crate::errors::FanCurveError::Unknown(format!("GUI error: {}", e)))?;
    Ok(())
}
