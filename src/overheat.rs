//! Overheat protection: forces every fan to full speed when the CPU or GPU
//! gets too hot, whatever the active curves say.

use log::{info, warn};

/// Temperature (°C) at or above which the fans are forced to 100%.
pub const OVERHEAT_ON_C: f32 = 90.0;
/// Temperature (°C) the hottest sensor must drop below to hand the fans back
/// to their curves. The gap keeps the fans from flapping around the limit.
pub const OVERHEAT_OFF_C: f32 = 80.0;

/// Tracks whether the overheat override is active, with hysteresis.
#[derive(Debug, Default)]
pub struct OverheatGuard {
    active: bool,
    /// Hottest reading at the last update: (°C, "CPU" or "GPU").
    hottest: Option<(f32, &'static str)>,
}

impl OverheatGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update from the latest readings in °C and return whether the fans
    /// must run at full speed. With no readings the state is left unchanged.
    pub fn update(&mut self, cpu: Option<f32>, gpu: Option<f32>) -> bool {
        let readings = cpu
            .map(|t| (t, "CPU"))
            .into_iter()
            .chain(gpu.map(|t| (t, "GPU")));
        let Some((hottest, sensor)) = readings.reduce(|a, b| if b.0 > a.0 { b } else { a }) else {
            return self.active;
        };
        self.hottest = Some((hottest, sensor));

        if !self.active && hottest >= OVERHEAT_ON_C {
            warn!(
                "Overheat: {} {:.1}°C reached {}°C; running all fans at 100% until below {}°C",
                sensor, hottest, OVERHEAT_ON_C, OVERHEAT_OFF_C
            );
            self.active = true;
        } else if self.active && hottest < OVERHEAT_OFF_C {
            info!(
                "Overheat cleared: {} at {:.1}°C; fans back on their curves",
                sensor, hottest
            );
            self.active = false;
        }
        self.active
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Hottest reading at the last update: (°C, "CPU" or "GPU").
    pub fn hottest(&self) -> Option<(f32, &'static str)> {
        self.hottest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_at_limit_and_clears_below_lower_limit() {
        let mut guard = OverheatGuard::new();
        assert!(!guard.update(Some(89.9), None));
        assert!(guard.update(Some(90.0), None));
        // Stays on between the two limits
        assert!(guard.update(Some(85.0), None));
        assert!(guard.update(Some(80.0), None));
        assert!(!guard.update(Some(79.9), None));
    }

    #[test]
    fn hottest_sensor_decides() {
        let mut guard = OverheatGuard::new();
        assert!(guard.update(Some(50.0), Some(92.0)));
        assert_eq!(guard.hottest(), Some((92.0, "GPU")));
        // CPU cool but GPU still above the lower limit
        assert!(guard.update(Some(40.0), Some(81.0)));
        assert!(!guard.update(Some(40.0), Some(70.0)));
    }

    #[test]
    fn missing_readings_keep_state() {
        let mut guard = OverheatGuard::new();
        assert!(!guard.update(None, None));
        guard.update(Some(95.0), None);
        assert!(guard.update(None, None));
        assert_eq!(guard.hottest(), Some((95.0, "CPU")));
    }
}
