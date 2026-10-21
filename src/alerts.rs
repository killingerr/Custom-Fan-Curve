//! States the daemon reports to users (overheat override, who controls the
//! fans) and the wording shown for them, shared by the GUI banner and the
//! desktop notification agent.

use crate::overheat::OVERHEAT_OFF_C;

/// Fan control state ids sent over DBus by GetFanControlState / FanControlChanged.
pub mod fan_control {
    /// The daemon holds system76-power's fan control claim.
    pub const CLAIMED: &str = "claimed";
    /// system76-power isn't running, so the daemon can't claim the fans and
    /// isn't writing them.
    pub const DIRECT: &str = "direct";
    /// system76-power can't hand over control, so its own curve is running and
    /// the daemon isn't writing the fans.
    pub const SHARED: &str = "shared";
    /// Another program holds the claim; the daemon isn't writing the fans.
    pub const HELD_BY_OTHER: &str = "held-by-other";
    /// system76-power refused the claim; its own curve is running.
    pub const REFUSED: &str = "refused";
    /// The daemon hasn't asked system76-power yet.
    pub const STARTING: &str = "starting";
}

/// A message for the user: (summary, body).
pub type Message = (String, String);

/// Message while the overheat override is active.
pub fn overheat_message(temperature: f64, sensor: &str) -> Message {
    (
        "Overheating: fans at full speed".to_string(),
        format!(
            "The {} reached {:.0}°C. All fans are running at 100% until it drops below \
             {:.0}°C. If this happens often, your fan curve may be too low for this workload.",
            sensor, temperature, OVERHEAT_OFF_C
        ),
    )
}

/// Message when the overheat override clears.
pub fn overheat_cleared_message(temperature: f64, sensor: &str) -> Message {
    (
        "Temperature back to normal".to_string(),
        format!(
            "The {} cooled to {:.0}°C. Fans are back on their fan curves.",
            sensor, temperature
        ),
    )
}

/// Message for a fan control state that keeps the user's curve from running
/// as set, or None when the curve is running normally.
pub fn fan_control_message(state: &str, detail: &str) -> Option<Message> {
    match state {
        fan_control::HELD_BY_OTHER => Some((
            "Fan curve paused".to_string(),
            "Another program has taken control of the fans, so your fan curve isn't \
             running. It resumes automatically when that program releases them."
                .to_string(),
        )),
        fan_control::REFUSED => Some((
            "Fan curve not running".to_string(),
            format!(
                "system76-power refused to hand over fan control ({}). Its own fan curve \
                 is running instead. The fan app keeps retrying.",
                detail
            ),
        )),
        fan_control::SHARED => Some((
            "Fan curve not running".to_string(),
            "This system76-power can't hand over fan control, so its own fan curve is \
             running instead of yours. Install a system76-power with fan control claim \
             support to use your fan curve."
                .to_string(),
        )),
        fan_control::DIRECT => Some((
            "Fan curve not running".to_string(),
            "system76-power isn't running, so the fan app can't take control of the fans \
             and your fan curve isn't running. It resumes automatically when \
             system76-power starts."
                .to_string(),
        )),
        _ => None,
    }
}

/// Message when the fan curve runs normally again after a problem.
pub fn fan_control_resolved_message() -> Message {
    (
        "Fan curve running".to_string(),
        "The fan app is controlling the fans again.".to_string(),
    )
}
