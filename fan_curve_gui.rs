use eframe::egui;
use std::sync::{Arc, Mutex};
use tokio::runtime::Builder;
use system76_power_zbus::PowerDaemonProxy;
use zbus::Connection;
use crate::fan::{FanCurve, FanPoint};
use log::{debug, error, info, warn};
use std::fs;

#[cfg(target_os = "windows")]
use winapi::um::winuser::{FindWindowA, ShowWindow, SW_MINIMIZE};
#[cfg(target_os = "linux")]
use x11rb::connection::Connection as X11Connection;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::*;

const DEFAULT_CURVE_FILE: &str = "/etc/system76-power/default_fan_curve";

pub struct FanCurveApp {
    fan_curves: Vec<FanCurve>,
    current_curve_index: usize,
    new_curve_name: String,
    client: Arc<PowerDaemonProxy<'static>>,
    runtime: tokio::runtime::Runtime,
    show_save_dialog: bool,
    default_curve_index: Option<usize>,
    set_default_status: Arc<Mutex<Option<String>>>,

}

enum SaveDialogResult {
    Cancel,
    Save(String),
}

impl FanCurveApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");
    
        let (client, fan_curves, current_curve_index, default_curve_index) = runtime.block_on(async {
            let connection = Connection::system().await.expect("Failed to connect to system bus");
            let client = PowerDaemonProxy::new(&connection).await.expect("Failed to create PowerDaemonProxy");
            let current_curve = client.get_fan_curve().await.unwrap_or_default();
            
            let mut fan_curves = vec![
                FanCurve::standard(),
                FanCurve::threadripper2(),
                FanCurve::hedt(),
                FanCurve::xeon(),
            ];
            
            // Load saved fan curves
            match client.load_all_fan_curves().await {
                Ok(saved_curves) => {
                    for (name, points) in saved_curves {
                        if !fan_curves.iter().any(|c| c.name() == name) {
                            let mut new_curve = FanCurve::new(name);
                            for (temp, duty) in points {
                                new_curve.add_point(i16::from(temp) * 100, u16::from(duty) * 100);
                            }
                            fan_curves.push(new_curve);
                        }
                    }
                },
                Err(e) => error!("Failed to load custom profiles: {}", e),
            }
    
            // Add debug logging
            debug!("Loaded {} custom profiles", fan_curves.len() - 4); // Subtract 4 for the default curves
    
            // Convert current_curve to Vec<FanPoint> for comparison
            let current_fan_points: Vec<FanPoint> = current_curve
                .into_iter()
                .map(|(temp, duty)| FanPoint::new(temp as i16, duty as u16))
                .collect();
    
            // Add the current curve if it doesn't match any predefined curves
            if !fan_curves.iter().any(|curve| curve.points() == current_fan_points.as_slice()) {
                let mut new_curve = FanCurve::new("Current".to_string());
                for point in &current_fan_points {
                    new_curve.add_point(point.temp, point.duty);
                }
                fan_curves.push(new_curve);
            }
            
            // Load the default curve name
            let default_curve_name = match fs::read_to_string(DEFAULT_CURVE_FILE) {
                Ok(name) => {
                    info!("Loaded default curve name: {}", name.trim());
                    Some(name.trim().to_string())
                },
                Err(e) => {
                    warn!("Failed to load default curve name: {}", e);
                    None
                }
            };
    
            let default_curve_index = default_curve_name.as_ref().and_then(|name| {
                fan_curves.iter().position(|curve| curve.name() == name)
            });
            
            // If a default curve is found, move it to the front of the list
            let current_curve_index = if let Some(index) = default_curve_index {
                let default_curve = fan_curves.remove(index);
                fan_curves.insert(0, default_curve);
                info!("Moved default curve to front of list");
                0
            } else {
                0 // Start with the first curve if no default is found
            };
    
            // Apply the default curve
            let default_curve = fan_curves[current_curve_index].clone();
            let curve_points: Vec<(u8, u8)> = default_curve.points()
                .iter()
                .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                .collect();
    
            let default_curve_name = default_curve.name().to_string();
            if let Err(e) = client.set_fan_curve(&curve_points).await {
                error!("Failed to apply default curve '{}' on startup: {}", default_curve_name, e);
            } else {
                info!("Applied default curve '{}' on startup", default_curve_name);
            }
    
            (Arc::new(client), fan_curves, current_curve_index, default_curve_index)
        });
    
        // Debug logging
        info!("Loaded fan curves:");
        for (i, curve) in fan_curves.iter().enumerate() {
            debug!("Curve {}: {} - {} points", i, curve.name(), curve.points().len());
            for (j, point) in curve.points().iter().enumerate() {
                debug!("  Point {}: temp = {:.2}°C, duty = {:.2}%", j, point.temp as f32 / 100.0, point.duty as f32 / 100.0);
            }
        }
        
        Self {
            fan_curves,
            current_curve_index,
            new_curve_name: String::new(),
            client,
            runtime,
            show_save_dialog: false,
            default_curve_index,
            set_default_status: Arc::new(Mutex::new(None)),
        }
    }

    fn set_default_curve(&mut self) {
        let current_index = self.current_curve_index;
        if let Some(curve) = self.fan_curves.get(current_index).cloned() {
            let curve_name = curve.name().to_string();
            let curve_points: Vec<(u8, u8)> = curve.points()
                .iter()
                .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                .collect();

            let client = self.client.clone();
            let status = self.set_default_status.clone();
            self.runtime.spawn(async move {
                match client.set_fan_curve_persistent(&curve_name, &curve_points).await {
                    Ok(_) => {
                        if let Err(e) = fs::write(DEFAULT_CURVE_FILE, &curve_name) {
                            error!("Failed to save default curve name: {}", e);
                            let mut status = status.lock().unwrap();
                            *status = Some(format!("Error saving default: {}", e));
                        } else {
                            info!("Set '{}' as the default fan curve", curve_name);
                            let mut status = status.lock().unwrap();
                            *status = Some(format!("'{}' set as default", curve_name));
                        }
                    },
                    Err(e) => {
                        error!("Failed to set default fan curve: {}", e);
                        let mut status = status.lock().unwrap();
                        *status = Some(format!("Error: {}", e));
                    },
                }
            });

            // Update the current state
            self.default_curve_index = Some(current_index);
        }
    }

    fn show_save_dialog(&mut self, ctx: &egui::Context) {
        let mut name = self.fan_curves[self.current_curve_index].name().to_string();
        let mut dialog_result = None;
        
        egui::Window::new("Save Fan Curve")
            .open(&mut self.show_save_dialog)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Curve Name:");
                    ui.text_edit_singleline(&mut name);
                });
                
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        dialog_result = Some(SaveDialogResult::Cancel);
                    }
                    if ui.button("Save").clicked() {
                        if !name.is_empty() {
                            dialog_result = Some(SaveDialogResult::Save(name.clone()));
                        }
                    }
                });
            });
            
         if let Some(result) = dialog_result {
            match result {
                SaveDialogResult::Cancel => {
                    self.show_save_dialog = false;
                }
                SaveDialogResult::Save(curve_name) => {
                    let curve = self.fan_curves[self.current_curve_index].clone();
                    let client = self.client.clone();
                    self.runtime.spawn(async move {
                        match client.set_fan_curve_persistent(&curve_name, &curve.points.iter()
                            .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                            .collect::<Vec<_>>()).await
                        {
                            Ok(_) => info!("Fan curve '{}' saved successfully", curve_name),
                            Err(e) => error!("Failed to save fan curve: {}", e),
                        }
                    });
                    self.show_save_dialog = false;
                }
            }
         }             
     }
     
    fn minimize_window(&self) {
        #[cfg(target_os = "windows")]
        unsafe {
            let window = FindWindowA(std::ptr::null(), "Fan Curve Control\0".as_ptr() as *const i8);
            if !window.is_null() {
                ShowWindow(window, SW_MINIMIZE);
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok((conn, screen_num)) = x11rb::connect(None) {
                let screen = &conn.setup().roots[screen_num];
                let root = screen.root;
                
                if let Ok(atom) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") {
                    if let Ok(atom_reply) = atom.reply() {
                        let event = ClientMessageEvent::new(
                            32,
                            root,
                            atom_reply.atom,
                            [0, 0, 0, 0, 0],
                        );

                        let _ = conn.send_event(
                            false,
                            root,
                            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                            &event,
                        );

                        let _ = conn.flush();
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // MacOS implementation would go here
            // This might involve using the Cocoa framework
        }
    }
}


impl eframe::App for FanCurveApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let mut style = (*ctx.style()).clone();
        style.spacing.window_margin = egui::style::Margin::same(0.0);
        style.spacing.button_padding = egui::vec2(2.0, 0.0);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            let title_bar_height = 32.0;
            let title_bar_rect = egui::Rect::from_min_size(
                ui.min_rect().min,
                egui::vec2(ui.available_width(), title_bar_height),
            );

            // Title bar
            ui.painter().rect_filled(title_bar_rect, 0.0, egui::Color32::from_rgb(60, 60, 60));
            ui.allocate_ui_at_rect(title_bar_rect, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("Fan Curve Control").color(egui::Color32::WHITE));
                });
            });

            // Make the title bar draggable
            let title_bar_response = ui.interact(title_bar_rect, egui::Id::new("title_bar"), egui::Sense::drag());
            if title_bar_response.dragged() {
                frame.drag_window();
            }

            // Window control buttons
            ui.allocate_ui_at_rect(title_bar_rect, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        frame.close();
                    }
                    if ui.button("□").clicked() {
                        frame.set_fullscreen(!frame.info().window_info.fullscreen);
                    }
                    if ui.button("_").clicked() {
                        self.minimize_window();
                    }
                });
            });
           

            // Main content
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(40, 40, 40))
                .inner_margin(egui::style::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.add_space(title_bar_height); // Space for the custom titlebar

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
                
                    let current_curve = &mut self.fan_curves[self.current_curve_index];
                
                    // Display and edit fan curve points
                    for (i, point) in current_curve.points_mut().iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Point {}:", i + 1));
                            let mut temp_f32 = point.temp as f32 / 100.0;
                            let mut duty_f32 = point.duty as f32 / 100.00;
                        
                            if ui.add(egui::DragValue::new(&mut temp_f32)
                                .speed(0.01)
                                .clamp_range(0.0..=100.0)
                                .prefix("Temp: ")
                                .suffix("°C")
                                .fixed_decimals(2)).changed() {
                                point.temp = (temp_f32 * 100.0) as i16;
                                debug!("Updated point {}: temp = {:.2}°C, duty = {:.2}%", i, point.temp as f32 / 100.0, point.duty as f32 / 100.0);
                            }
                          
                            if ui.add(egui::DragValue::new(&mut duty_f32)
                                .speed(0.01)
                                .clamp_range(0.0..=100.0)
                                .prefix("Speed: ")
                                .suffix("%")
                                .fixed_decimals(2)).changed() {
                                point.duty = (duty_f32 * 100.0) as u16;
                                debug!("Updated point {}: temp = {:.2}°C, duty = {:.2}%", i, point.temp as f32 / 100.0, point.duty as f32 / 100.0);
                            }
                        });
                    }
                
                    // Add and remove points
                    ui.horizontal(|ui| {
                        if ui.button("Add Point").clicked() {
                            let new_temp = current_curve.last_point().map_or(0, |p| p.temp + 500);
                            let new_duty = current_curve.last_point().map_or(0, |p| p.duty + 1000).min(10000);
                            current_curve.add_point(new_temp, new_duty);
                        }
                        
                        if ui.button("Remove Last Point").clicked() && current_curve.len() > 2 {
                            current_curve.remove_last_point();
                        }
                    });
                    
                    // Create new curve
                    ui.horizontal(|ui| {
                        ui.label("New Curve Name:");
                        ui.text_edit_singleline(&mut self.new_curve_name);
                        if ui.button("Create New Curve").clicked() && !self.new_curve_name.is_empty() {
                            self.fan_curves.push(FanCurve::new(self.new_curve_name.clone()));
                            self.current_curve_index = self.fan_curves.len() - 1;
                            self.new_curve_name.clear();
                        }
                    });
                
                    // Apply changes and Save curve
                    ui.horizontal(|ui| {
                        if ui.button("Apply Curve").clicked() {
                            let client = self.client.clone();
                            let fan_curve = self.fan_curves[self.current_curve_index].clone();
                            self.runtime.spawn(async move {
                                let curve_data: Vec<(u8, u8)> = fan_curve.points.iter()
                                    .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                                    .collect();
                                if let Err(e) = client.set_fan_curve(&curve_data).await {
                                    error!("Failed to set fan curve: {}", e);
                                } else {
                                    info!("Fan curve applied successfully");
                                }
                            });
                        }
                        
                        if ui.button("Save Curve").clicked() {
                           self.show_save_dialog = true;
                        }
                    });
                    
                    //
                    let set_default_button = ui.add_enabled(
                        Some(self.current_curve_index) != self.default_curve_index,
                        egui::Button::new("Set as default")
                    );
                    
                    if set_default_button.clicked() {
                        self.set_default_curve();
                    }
                    
                    // Display status message
                    if let Some(status) = self.set_default_status.lock().unwrap().as_ref() {
                        ui.label(status);
                    }
                    
                });
                
        });
        
        // Show the save dialog if it's activated
        if self.show_save_dialog {
            self.show_save_dialog(ctx);
        }
    }
}

