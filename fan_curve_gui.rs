use eframe::egui;
use std::sync::Arc;
use tokio::runtime::Builder;
use system76_power_zbus::PowerDaemonProxy;
use zbus::Connection;
use crate::fan::{FanCurve, FanPoint};

pub struct FanCurveApp {
    fan_curves: Vec<FanCurve>,
    current_curve_index: usize,
    new_curve_name: String,
    client: Arc<PowerDaemonProxy<'static>>,
    runtime: tokio::runtime::Runtime,
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
            
            //Convert current_curve toe Vec<FanPoint> for comparison
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
        
        //Debug logging
        println!("Loaded fan curves:");
        for (i, curve) in fan_curves.iter().enumerate() {
            println!("Curve {}: {} - {} points", i, curve.name(), curve.points().len());
            for (j, point) in curve.points().iter().enumerate() {
                println!("  Point {}: temp = {:.2}°C, duty = {:.2}%", j, point.temp as f32 / 100.0, point.duty as f32 / 100.0);
            }
        }
        
        Self {
            fan_curves,
            current_curve_index: 0,
            new_curve_name: String::new(),
            client,
            runtime,
        }
    }
    
    async fn save_fan_curve(&self, curve: &FanCurve) -> Result<(), zbus::Error> {
        let curve_data: Vec<(u8, u8)> = curve.points.iter()
            .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
            .collect();
        self.client.set_fan_curve(&curve_data).await
    }
}

impl eframe::App for FanCurveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Fan Curve Control");
            
            egui::ComboBox::from_label("Select Fan Curve")
                .selected_text(self.fan_curves[self.current_curve_index].name())
                .show_ui(ui, |ui| {
                    for (index, curve) in self.fan_curves.iter().enumerate() {
                        ui.selectable_value(&mut self.current_curve_index, index, curve.name());
                    }
                });
            
            let current_curve = &mut self.fan_curves[self.current_curve_index];
            
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
                        // Debug output after modifying a point
                        println!("Updated point {}: temp = {:.2}°C, duty = {:.2}%", i, point.temp as f32 / 100.0, point.duty as f32 / 100.0);
                      }
                      
                    if ui.add(egui::DragValue::new(&mut duty_f32)
                        .speed(0.01)
                        .clamp_range(0.0..=100.0)
                        .prefix("Speed: ")
                        .suffix("%")
                        .fixed_decimals(2)).changed() {
                        point.duty = (duty_f32 * 100.0) as u16;
                        // Debug output after modifying a point
                        println!("Updated point {}: temp = {:.2}°C, duty = {:.2}%", i, point.temp as f32 / 100.0, point.duty as f32 / 100.0);
                      }
                });
            }
            
            if ui.button("Add Point").clicked() {
                let new_temp = current_curve.last_point().map_or(0, |p| p.temp + 500);
                let new_duty = current_curve.last_point().map_or(0, |p| p.duty + 1000).min(10000);
                current_curve.add_point(new_temp, new_duty);
            }
            
            if ui.button("Remove Last Point").clicked() && current_curve.len() > 2 {
                current_curve.remove_last_point();
            }
            
            ui.horizontal(|ui| {
                ui.label("New Curve Name:");
                ui.text_edit_singleline(&mut self.new_curve_name);
                if ui.button("Create New Curve").clicked() && !self.new_curve_name.is_empty() {
                    self.fan_curves.push(FanCurve::new(self.new_curve_name.clone()));
                    self.current_curve_index = self.fan_curves.len() - 1;
                    self.new_curve_name.clear();
                }
            });
            
            if ui.button("Apply Changes").clicked() {
                let client = self.client.clone();
                let fan_curve = self.fan_curves[self.current_curve_index].clone();
                self.runtime.spawn(async move {
                    let curve_data: Vec<(u8, u8)> = fan_curve.points.iter()
                        .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                        .collect();
                    if let Err(e) = client.set_fan_curve(&curve_data).await {
                        eprintln!("Failed to set fan curve: {}", e);
                    } else {
                        println!("Fan curve applied successfully");
                    }
                });
            }
            
            if ui.button("Save Curve").clicked() {
                let client = self.client.clone();
                let fan_curve = self.fan_curves[self.current_curve_index].clone();
                self.runtime.spawn(async move {
                    let curve_data: Vec<(u8, u8)> = fan_curve.points.iter()
                        .map(|point| ((point.temp / 100) as u8, (point.duty / 100) as u8))
                        .collect();
                    if let Err(e) = client.set_fan_curve_persistent(&curve_data).await {
                        eprintln!("Failed to save fan curve: {}", e);
                    } else {
                        println!("Fan curve saved successfully");
                    }
                });
           }
        });
    }
}
