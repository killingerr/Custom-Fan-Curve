use crate::errors::Result;
use log::{debug, info, warn};
use zbus::blocking::Connection as BlockingConnection;

/// Wrapper for interacting with the Thelio IO daemon, when available.
///
/// This implementation is intentionally conservative: it only detects whether
/// a Thelio IO service appears to be present on the system bus and provides
/// placeholder methods that can be filled in with the concrete interface once
/// the DBus schema is finalized.
///
/// Behavior:
/// - If the service is unavailable or disabled via env toggle, calls are no-ops.
/// - All methods return `Ok(())` on no-op to avoid breaking existing flows.
pub struct ThelioIoClient {
    enabled: bool,
    #[allow(dead_code)]
    service_name: String,
    // We keep only an async-capable connection reference pattern for now.
    // Concrete proxies can be added once the interface is known.
    is_available: bool,
}

impl ThelioIoClient {
    /// Environment variable to control usage.
    const ENV_ENABLE: &'static str = "FAN_APP_ENABLE_THELIO_IO";
    /// Default DBus service name used by the Thelio IO daemon.
    /// This is a best-effort guess and can be adjusted when the exact name is known.
    const DEFAULT_SERVICE: &'static str = "com.system76.ThelioIo";

    /// Attempt to construct a new client.
    ///
    /// If disabled via env or the service is not present, the client will be
    /// created in a disabled state and act as a no-op.
    pub fn new() -> Result<Self> {
        let enabled = std::env::var(Self::ENV_ENABLE)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let service_name = std::env::var("FAN_APP_THELIO_IO_SERVICE")
            .unwrap_or_else(|_| Self::DEFAULT_SERVICE.to_string());

        if !enabled {
            debug!(
                "Thelio IO integration disabled (set {}=1 to enable)",
                Self::ENV_ENABLE
            );
            return Ok(Self {
                enabled: false,
                service_name,
                is_available: false,
            });
        }

        // Best-effort check for service presence on the system bus using a blocking probe
        // to avoid tying the daemon's startup to async timing here.
        let is_available = Self::probe_service_blocking(&service_name);

        if is_available {
            info!("Thelio IO service detected: {}", service_name);
        } else {
            warn!(
                "Thelio IO integration enabled but service not found: {}",
                service_name
            );
        }

        Ok(Self {
            enabled: true,
            service_name,
            is_available,
        })
    }

    fn probe_service_blocking(service_name: &str) -> bool {
        // Use a short-lived blocking connection to check for name owner.
        if let Ok(conn) = BlockingConnection::system() {
            if let Ok(proxy) = zbus::blocking::Proxy::new(
                &conn,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
            ) {
                // Ask DBus if the name has an owner; true implies the service is up.
                let result: std::result::Result<bool, zbus::Error> =
                    proxy.call("NameHasOwner", &(service_name));
                return result.unwrap_or(false);
            }
        }
        false
    }

    /// Returns true if the client is enabled and the service appears available.
    pub fn available(&self) -> bool {
        self.enabled && self.is_available
    }

    /// Set fan duty on a given channel (0-100%).
    ///
    /// Currently a no-op placeholder until the DBus interface is wired.
    pub async fn set_fan_duty(&self, _channel: u8, _duty_percent: u8) -> Result<()> {
        if !self.available() {
            return Ok(());
        }
        // TODO: Replace with concrete DBus proxy/method when interface is available.
        Ok(())
    }

    /// Read back current fan RPM for a given channel.
    ///
    /// Returns None when unavailable or unimplemented.
    pub async fn get_fan_rpm(&self, _channel: u8) -> Result<Option<u32>> {
        if !self.available() {
            return Ok(None);
        }
        // TODO: Replace with concrete DBus proxy/method when interface is available.
        Ok(None)
    }

    /// Read a chassis temperature sensor, if exposed by Thelio IO.
    pub async fn get_temperature_c(&self, _sensor: &str) -> Result<Option<f32>> {
        if !self.available() {
            return Ok(None);
        }
        // TODO: Replace with concrete DBus proxy/method when interface is available.
        Ok(None)
    }
}
