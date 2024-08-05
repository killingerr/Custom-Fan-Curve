use eframe::egui;
use std::sync::Arc;
use tokio::runtime::Builder;
use system76_power_zbus::PowerDaemonProxy;
use zbus::Connection;
use crate::fan::{FanCurve, FanPoint};
use log::{debug, error, info};

#[cfg(target_os = "windows")]
use winapi::um::winuser::{FindWindowA, ShowWindow, SW_MINIMIZE};
#[cfg(target_os = "linux")]
use x11rb::connection::Connection as X11Connection;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::*;

pub struct FanCurveApp {
    fan_curves: Vec<FanCurve>,
    current_curve_index: usize,
    new_curve_name: String,
    client: Arc<PowerDaemonProxy<'static>>,
    runtime: tokio::runtime::Runtime,
    show_save_dialog: bool,
    default_curve_index: Option<usize>,

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

        let (client, fan_curves) = runtime.block_on(async {
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
                    debug!("Saved curves received: {}", saved_curves.len());
                    for (name, points) in saved_curves {
                        debug!("Processing curve: {} with {} points", name, points.len());
                        if !fan_curves.iter().any(|c| c.name() == name) {
                            let mut new_curve = FanCurve::new(name.clone());
                            for (temp, duty) in points {
                                new_curve.add_point(i16::from(temp) * 100, u16::from(duty) * 100);
                            }
                            fan_curves.push(new_curve);
                            debug!("Added new curve: {}", name);
                        } else {
                            debug!("Skipped duplicate curve: {}", name);
                        }   
                    }
                },
                Err(e) => eprintln!("Failed to load saved fan curves: {}", e),
            }
            
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
            
            (Arc::new(client), fan_curves)
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
            current_curve_index: 0,
            new_curve_name: String::new(),
            client,
            runtime,
            show_save_dialog: false,
            default_curve_index: None,
        }
    }

    fn set_default_curve(&mut self) {
        let current_index = self.current_curve_index;
        if let Some(curve) = self.fan_curves.get(current_index) {
            let curve_name = curve.name().to_string();
            let curve_points: Vec<(u8, u8)> = curve.points()
                .iter()
                .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                .collect();

            let client = self.client.clone();
            self.runtime.spawn(async move {
                match client.set_fan_curve_persistent(&curve_name, &curve_points).await {
                    Ok(_) => {
                        info!("Set '{}' as the default fan curve", curve_name);
                    },
                    Err(e) => error!("Failed to set default fan curve: {}", e),
                }
            });

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
            let mut title_bar_rect = ui.max_rect();
            title_bar_rect.max.y = title_bar_rect.min.y + title_bar_height;
            let title_bar_response = ui.allocate_rect(title_bar_rect, egui::Sense::click_and_drag());

            if title_bar_response.dragged() && !title_bar_response.clicked() {
                frame.drag_window();
            }

            // Draw title bar
            ui.painter().rect_filled(title_bar_rect, 0.0, egui::Color32::from_rgb(60, 60, 60));
            
            // Title
            ui.painter().text(
                title_bar_rect.left_center() + egui::vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Fan Curve Control",
                egui::FontId::proportional(18.0),
                egui::Color32::WHITE,
            );

            // Window control buttons
            let button_size = egui::vec2(title_bar_height, title_bar_height);
            let button_margin = 2.0;

            // Close button
            let close_rect = egui::Rect::from_min_size(
                egui::pos2(title_bar_rect.right() - button_size.x, title_bar_rect.top()),
                button_size
            );
            let close_response = ui.allocate_rect(close_rect, egui::Sense::click());
            if close_response.clicked() {
                frame.close();
            }
            ui.painter().rect_filled(close_rect, 0.0, egui::Color32::from_rgb(200, 80, 80));
            ui.painter().text(close_rect.center(), egui::Align2::CENTER_CENTER, "X", egui::FontId::proportional(16.0), egui::Color32::WHITE);

            // Maximize button
            let maximize_rect = egui::Rect::from_min_size(
                egui::pos2(close_rect.left() - button_size.x - button_margin, title_bar_rect.top()),
                button_size
            );
            let maximize_response = ui.allocate_rect(maximize_rect, egui::Sense::click());
            if maximize_response.clicked() {
                frame.set_fullscreen(!frame.info().window_info.fullscreen);
            }
            ui.painter().rect_filled(maximize_rect, 0.0, egui::Color32::from_rgb(80, 80, 80));
            ui.painter().text(maximize_rect.center(), egui::Align2::CENTER_CENTER, "□", egui::FontId::proportional(16.0), egui::Color32::WHITE);

            // Minimize button
            let minimize_rect = egui::Rect::from_min_size(
                egui::pos2(maximize_rect.left() - button_size.x - button_margin, title_bar_rect.top()),
                button_size
            );
            let minimize_response = ui.allocate_rect(minimize_rect, egui::Sense::click());
            if minimize_response.clicked() {
                self.minimize_window();
            }
            ui.painter().rect_filled(minimize_rect, 0.0, egui::Color32::from_rgb(80, 80, 80));
            ui.painter().text(minimize_rect.center(), egui::Align2::CENTER_CENTER, "_", egui::FontId::proportional(16.0), egui::Color32::WHITE);

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
                        if ui.button("Apply Changes").clicked() {
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
                    
                    // Add "Set as Default" button
                    if ui.button("Set as Default").clicked() {
                        self.set_default_curve();
                    }
                    
                });
                
        });
        
        // Show the save dialog if it's activated
        if self.show_save_dialog {
            self.show_save_dialog(ctx);
        }
    }
}

