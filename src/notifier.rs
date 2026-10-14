//! Desktop notification agent (`fan-curve-app notify`).
//!
//! The daemon runs as root outside any desktop session, so it can't show
//! notifications itself. This agent runs in the user's session (started at
//! login through XDG autostart), listens for the daemon's OverheatChanged and
//! FanControlChanged signals on the system bus, and shows desktop
//! notifications through org.freedesktop.Notifications on the session bus.

use crate::alerts::{self, fan_control, Message};
use crate::errors::Result;
use crate::{DBUS_INTERFACE_NAME, DBUS_OBJECT_PATH, DBUS_SERVICE_NAME};
use futures_util::stream::{self, StreamExt};
use log::{debug, info, warn};
use std::collections::HashMap;
use zbus::{Connection, MatchRule, MessageStream, MessageType};
use zvariant::Value;

const APP_NAME: &str = "Fan Curve Control";

/// freedesktop notification urgency levels
const URGENCY_NORMAL: u8 = 1;
const URGENCY_CRITICAL: u8 = 2;

struct Notifier {
    session: Connection,
    /// Notification ids, so updates replace the previous notification of the
    /// same kind instead of stacking up.
    overheat_id: u32,
    fan_control_id: u32,
    overheating: bool,
    /// Fan control state currently being warned about, if any.
    fan_control_problem: Option<String>,
}

impl Notifier {
    /// Show a notification, replacing `replaces` if non-zero. Returns its id.
    async fn notify(&self, replaces: u32, message: Message, icon: &str, urgency: u8) -> u32 {
        let (summary, body) = message;
        let mut hints: HashMap<&str, Value> = HashMap::new();
        hints.insert("urgency", Value::U8(urgency));
        let actions: Vec<&str> = Vec::new();

        let reply = self
            .session
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    APP_NAME,
                    replaces,
                    icon,
                    summary.as_str(),
                    body.as_str(),
                    actions,
                    hints,
                    -1i32,
                ),
            )
            .await;
        match reply.and_then(|r| r.body::<u32>()) {
            Ok(id) => id,
            Err(e) => {
                warn!("Failed to show notification '{}': {}", summary, e);
                replaces
            }
        }
    }

    async fn on_overheat(&mut self, active: bool, temperature: f64, sensor: &str) {
        if active {
            // Critical: stays on screen until the user dismisses it.
            let message = alerts::overheat_message(temperature, sensor);
            self.overheat_id = self
                .notify(
                    self.overheat_id,
                    message,
                    "dialog-warning",
                    URGENCY_CRITICAL,
                )
                .await;
            self.overheating = true;
        } else if self.overheating {
            let message = alerts::overheat_cleared_message(temperature, sensor);
            self.overheat_id = self
                .notify(
                    self.overheat_id,
                    message,
                    "dialog-information",
                    URGENCY_NORMAL,
                )
                .await;
            self.overheating = false;
        }
    }

    async fn on_fan_control(&mut self, state: &str, detail: &str) {
        match alerts::fan_control_message(state, detail) {
            Some(message) => {
                if self.fan_control_problem.as_deref() == Some(state) {
                    return;
                }
                self.fan_control_id = self
                    .notify(
                        self.fan_control_id,
                        message,
                        "dialog-warning",
                        URGENCY_NORMAL,
                    )
                    .await;
                self.fan_control_problem = Some(state.to_string());
            }
            None if state != fan_control::STARTING && self.fan_control_problem.is_some() => {
                let message = alerts::fan_control_resolved_message();
                self.fan_control_id = self
                    .notify(
                        self.fan_control_id,
                        message,
                        "dialog-information",
                        URGENCY_NORMAL,
                    )
                    .await;
                self.fan_control_problem = None;
            }
            None => {}
        }
    }

    /// Ask the daemon for its current state, e.g. at startup or after the
    /// daemon (re)starts, so problems that began earlier are still reported.
    async fn sync(&mut self, system: &Connection) {
        let call = |method: &'static str| {
            system.call_method(
                Some(DBUS_SERVICE_NAME),
                DBUS_OBJECT_PATH,
                Some(DBUS_INTERFACE_NAME),
                method,
                &(),
            )
        };

        match call("GetOverheat")
            .await
            .and_then(|r| r.body::<(bool, f64, String)>())
        {
            Ok((active, temperature, sensor)) => {
                self.on_overheat(active, temperature, &sensor).await
            }
            Err(e) => debug!("Could not get overheat state from the daemon: {}", e),
        }
        match call("GetFanControlState")
            .await
            .and_then(|r| r.body::<(String, String)>())
        {
            Ok((state, detail)) => self.on_fan_control(&state, &detail).await,
            Err(e) => debug!("Could not get fan control state from the daemon: {}", e),
        }
    }
}

/// Run the notification agent until the session ends.
pub async fn run() -> Result<()> {
    let system = Connection::system().await?;
    let session = Connection::session().await?;

    let daemon_signals = MessageStream::for_match_rule(
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(DBUS_INTERFACE_NAME)?
            .path(DBUS_OBJECT_PATH)?
            .build(),
        &system,
        None,
    )
    .await?;
    // Fires when the daemon starts or stops, so state can be re-read.
    let daemon_owner = MessageStream::for_match_rule(
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .add_arg(DBUS_SERVICE_NAME)?
            .build(),
        &system,
        None,
    )
    .await?;

    let mut notifier = Notifier {
        session,
        overheat_id: 0,
        fan_control_id: 0,
        overheating: false,
        fan_control_problem: None,
    };
    info!("Notification agent started");
    notifier.sync(&system).await;

    let mut events = stream::select(daemon_signals, daemon_owner);
    while let Some(msg) = events.next().await {
        let Ok(msg) = msg else { continue };
        let Some(member) = msg.member() else { continue };
        match member.as_str() {
            "OverheatChanged" => {
                if let Ok((active, temperature, sensor)) = msg.body::<(bool, f64, String)>() {
                    notifier.on_overheat(active, temperature, &sensor).await;
                }
            }
            "FanControlChanged" => {
                if let Ok((state, detail)) = msg.body::<(String, String)>() {
                    notifier.on_fan_control(&state, &detail).await;
                }
            }
            "NameOwnerChanged" => {
                if let Ok((_, _, new_owner)) = msg.body::<(String, String, String)>() {
                    if !new_owner.is_empty() {
                        info!("Fan curve daemon started; reading its state");
                        notifier.sync(&system).await;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
