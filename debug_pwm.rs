use fan_curve_app::fan_detector::FanDetector;
use fan_curve_app::fan_monitor::FanMonitor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("🔍 PWM Debug Tool - Testing 100% Fan Control");
    println!("=============================================\n");

    // Initialize fan detector
    let mut fan_detector = FanDetector::new();
    match fan_detector.initialize() {
        Ok(()) => {
            println!("✅ Fan detector initialized successfully");
            println!("   Found {} fans", fan_detector.fan_count());

            // List all detected fans
            for fan in fan_detector.get_fans() {
                println!(
                    "   📍 Fan {}: {} at {}",
                    fan.fan_number, fan.fan_label, fan.hwmon_path
                );
            }
        }
        Err(e) => {
            println!("❌ Failed to initialize fan detector: {}", e);
            return Err(e.into());
        }
    }

    println!("\n🧪 Testing PWM Control at 100% (PWM value 255)");
    println!("===============================================");

    // Test setting PWM to 255 (100%)
    match fan_detector.set_duty(Some(255)) {
        Ok(()) => {
            println!("✅ Successfully set PWM to 255 (100%)");
        }
        Err(e) => {
            println!("❌ Failed to set PWM to 255: {}", e);
        }
    }

    // Verify the PWM values were actually set
    println!("\n🔍 Verifying PWM values...");
    if let Err(e) = fan_detector.verify_pwm_values() {
        println!("⚠️  PWM verification failed: {}", e);
    }

    // Test with fan monitor
    println!("\n🧪 Testing with Fan Monitor (100% duty curve)");
    println!("=============================================");

    let mut monitor = FanMonitor::new();
    monitor.initialize()?;

    // Create a 100% duty curve
    let mut test_curve = fan_curve_app::fan::FanCurve::new("Debug 100%".to_string());
    test_curve.add_point(0, 10000); // 100% at 0°C
    test_curve.add_point(50, 10000); // 100% at 50°C
    test_curve.add_point(100, 10000); // 100% at 100°C
    monitor.set_fan_curve(test_curve);

    // Test at 50°C (should trigger 100% duty)
    let test_temp = 50.0;
    println!("🌡️  Testing at {}°C (should trigger 100% duty)", test_temp);

    if let Err(e) = monitor.apply_fan_curve(test_temp).await {
        println!("❌ Failed to apply fan curve: {}", e);
    }

    // Verify again
    println!("\n🔍 Final PWM verification...");
    if let Err(e) = fan_detector.verify_pwm_values() {
        println!("⚠️  Final PWM verification failed: {}", e);
    }

    println!("\n✅ Debug test completed!");
    println!("\n💡 If fans are not running at 100%, check:");
    println!("   1. Are you running as root/sudo?");
    println!("   2. Are the PWM files writable?");
    println!("   3. Is the correct hwmon device detected?");
    println!("   4. Are the fans physically connected?");

    Ok(())
}
