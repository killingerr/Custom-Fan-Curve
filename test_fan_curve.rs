use fan_curve_app::fan_monitor::FanMonitor;
use fan_curve_app::fan::FanCurve;
use fan_curve_app::logging;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::setup(1); // INFO level
    
    println!("=== Testing Fan Curve Application ===");
    
    // Create fan monitor
    let mut fan_monitor = FanMonitor::new();
    
    // Get current data
    println!("Getting current fan data...");
    let current_data = fan_monitor.get_current_fan_data_direct()?;
    println!("Current temperature: {:.1}°C", current_data.temperature);
    println!("Current fan duty: {:.1}%", current_data.fan_duty as f32 / 100.0);
    println!("Current PWM: {}", current_data.fan_duty);
    
    // Create Standard curve
    let standard_curve = FanCurve::standard();
    println!("\nStandard curve points:");
    for (i, point) in standard_curve.points().iter().enumerate() {
        println!("  Point {}: {}°C -> {:.1}%", i + 1, point.temp, point.duty as f32 / 100.0);
    }
    
    // Apply the curve
    println!("\nApplying Standard curve at {:.1}°C...", current_data.temperature);
    let result = fan_monitor.apply_fan_curve_from_gui(&standard_curve, current_data.temperature);
    
    match result {
        Ok(_) => println!("✅ Fan curve applied successfully!"),
        Err(e) => println!("❌ Failed to apply fan curve: {}", e),
    }
    
    // Check PWM after application
    println!("\nChecking PWM after application...");
    let new_data = fan_monitor.get_current_fan_data_direct()?;
    println!("New PWM: {}", new_data.fan_duty);
    
    Ok(())
}
