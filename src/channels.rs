//! Per-channel fan control for Thelio Io boards.
//!
//! The Thelio Io kernel drivers expose up to four PWM channels. On Thelio Io 2
//! (`system76_thelio_io`) the driver fixes what each channel is wired to:
//! pwm1 = CPU Fan, pwm2 = Intake Fan, pwm3 = GPU Fan, pwm4 = Aux Fan.
//! Each channel can follow its own curve and temperature source.

use crate::fan::{FanCurve, FanCurveConfig};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How often to look for GPU temperature sources again while none are found,
/// e.g. because the daemon started before the GPU driver loaded.
const GPU_RESCAN_INTERVAL: Duration = Duration::from_secs(5);
/// How long to stop calling nvidia-smi after it fails.
const NVIDIA_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// hwmon name of the Thelio Io 2 driver, whose channel wiring is fixed.
const THELIO_IO_2: &str = "system76_thelio_io";
/// hwmon name of the original Thelio Io driver.
const THELIO_IO_1: &str = "system76_io";

/// Temperature that drives a fan channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TempSource {
    Cpu,
    Gpu,
    /// The hotter of the CPU and GPU, as system76-power uses for every fan.
    Hottest,
}

/// User override for one PWM channel, stored in the config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAssignment {
    /// PWM channel number (1-4)
    pub channel: u8,
    pub source: TempSource,
    /// Curve name to follow; None follows the active curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<String>,
}

/// Fixed channel labels on Thelio Io 2.
fn thelio_io_2_label(channel: u8) -> Option<&'static str> {
    match channel {
        1 => Some("CPU Fan"),
        2 => Some("Intake Fan"),
        3 => Some("GPU Fan"),
        4 => Some("Aux Fan"),
        _ => None,
    }
}

/// Source a channel uses when the config has no assignment for it.
fn default_source(fixed_wiring: bool, channel: u8) -> TempSource {
    if !fixed_wiring {
        // Wiring unknown: drive every fan from the hottest sensor, like system76-power.
        return TempSource::Hottest;
    }
    match channel {
        1 => TempSource::Cpu,
        3 => TempSource::Gpu,
        _ => TempSource::Hottest,
    }
}

/// Temperature for `source`, falling back to whichever sensor is available.
fn source_temp(source: TempSource, cpu: Option<f32>, gpu: Option<f32>) -> Option<f32> {
    match source {
        TempSource::Cpu => cpu.or(gpu),
        TempSource::Gpu => gpu.or(cpu),
        TempSource::Hottest => match (cpu, gpu) {
            (Some(c), Some(g)) => Some(c.max(g)),
            (c, g) => c.or(g),
        },
    }
}

/// The curve and temperature source a channel follows.
fn resolve(
    config: &FanCurveConfig,
    active_index: usize,
    fixed_wiring: bool,
    channel: u8,
) -> (TempSource, &FanCurve, Option<&str>) {
    let active = &config.curves[active_index.min(config.curves.len() - 1)];
    let Some(assignment) = config.channels.iter().find(|a| a.channel == channel) else {
        return (default_source(fixed_wiring, channel), active, None);
    };
    match assignment.curve.as_deref() {
        None => (assignment.source, active, None),
        Some(name) => match config.curves.iter().find(|c| c.name() == name) {
            Some(curve) => (assignment.source, curve, None),
            // Missing curve: report the name so the caller can warn about it.
            None => (assignment.source, active, Some(name)),
        },
    }
}

/// Duty (0-10000) for `channel`, or None when no temperature is available.
pub fn channel_duty(
    config: &FanCurveConfig,
    active_index: usize,
    fixed_wiring: bool,
    channel: u8,
    cpu: Option<f32>,
    gpu: Option<f32>,
) -> Option<u16> {
    if config.curves.is_empty() {
        return None;
    }
    let (source, curve, _) = resolve(config, active_index, fixed_wiring, channel);
    source_temp(source, cpu, gpu).map(|t| curve.calculate_duty_for_temperature_celsius(t))
}

/// One PWM channel found on the board.
#[derive(Debug, Clone)]
pub struct IoChannel {
    pub number: u8,
    pub label: String,
}

/// A Thelio Io board's hwmon device and its PWM channels.
pub struct IoBoard {
    hwmon: PathBuf,
    /// True on Thelio Io 2, whose channel wiring is fixed by the driver.
    fixed_wiring: bool,
    channels: Vec<IoChannel>,
    /// Curve names already warned about as missing, to avoid repeating it every tick.
    warned_curves: Vec<String>,
    write_failing: bool,
}

impl IoBoard {
    /// Find a Thelio Io board in /sys/class/hwmon.
    pub fn discover() -> Option<Self> {
        let entries = fs::read_dir("/sys/class/hwmon").ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(name) = fs::read_to_string(path.join("name")) else {
                continue;
            };
            let name = name.trim();
            if name != THELIO_IO_2 && name != THELIO_IO_1 {
                continue;
            }

            let fixed_wiring = name == THELIO_IO_2;
            let channels: Vec<IoChannel> = (1..=4)
                .filter(|n| path.join(format!("pwm{}", n)).exists())
                .map(|n| IoChannel {
                    number: n,
                    label: read_label(&path, n, fixed_wiring),
                })
                .collect();
            if channels.is_empty() {
                continue;
            }

            info!(
                "Found {} at {} with channels: {}",
                name,
                path.display(),
                channels
                    .iter()
                    .map(|c| format!("pwm{} ({})", c.number, c.label))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Some(Self {
                hwmon: path,
                fixed_wiring,
                channels,
                warned_curves: Vec::new(),
                write_failing: false,
            });
        }
        None
    }

    pub fn channels(&self) -> &[IoChannel] {
        &self.channels
    }

    /// Compute and write every channel's duty, or 100% on every channel when
    /// `full_speed` is set. Returns the (channel, duty, pwm) values written.
    pub fn apply(
        &mut self,
        config: &FanCurveConfig,
        active_index: usize,
        cpu: Option<f32>,
        gpu: Option<f32>,
        full_speed: bool,
    ) -> Vec<(u8, u16, u8)> {
        if config.curves.is_empty() {
            return Vec::new();
        }

        // Like system76-power, pwm1_enable switches the whole board to manual.
        // Some boards have no enable file; the writes below still take effect.
        let _ = fs::write(self.hwmon.join("pwm1_enable"), "1");

        let mut written = Vec::new();
        let mut failed = None;
        for channel in &self.channels {
            let (source, _, missing) =
                resolve(config, active_index, self.fixed_wiring, channel.number);
            if let Some(name) = missing {
                if !self.warned_curves.iter().any(|n| n == name) {
                    warn!(
                        "pwm{}: curve '{}' not found; following the active curve",
                        channel.number, name
                    );
                    self.warned_curves.push(name.to_string());
                }
            }

            let duty = if full_speed {
                Some(10_000)
            } else {
                channel_duty(
                    config,
                    active_index,
                    self.fixed_wiring,
                    channel.number,
                    cpu,
                    gpu,
                )
            };
            let Some(duty) = duty else {
                continue;
            };
            let pwm = ((u32::from(duty) * 255) / 10_000) as u8;
            debug!(
                "pwm{} ({}): {:?} -> duty {} -> PWM {}",
                channel.number, channel.label, source, duty, pwm
            );

            let path = self.hwmon.join(format!("pwm{}", channel.number));
            match fs::write(&path, pwm.to_string()) {
                Ok(()) => written.push((channel.number, duty, pwm)),
                Err(e) => failed = Some((path, e)),
            }
        }

        match failed {
            Some((path, e)) if !self.write_failing => {
                warn!("Failed to write {}: {}", path.display(), e);
                self.write_failing = true;
            }
            None if self.write_failing => {
                info!("PWM writes are succeeding again");
                self.write_failing = false;
            }
            _ => {}
        }

        written
    }

    /// Return the board to automatic (firmware) fan control.
    pub fn set_automatic(&self) {
        if let Err(e) = fs::write(self.hwmon.join("pwm1_enable"), "2") {
            debug!("Could not write pwm1_enable: {}", e);
        }
    }

    /// RPM of each channel's fan, as (channel, rpm, label).
    pub fn read_speeds(&self) -> Vec<(u8, u16, String)> {
        self.channels
            .iter()
            .filter_map(|c| {
                let raw =
                    fs::read_to_string(self.hwmon.join(format!("fan{}_input", c.number))).ok()?;
                let rpm = raw.trim().parse().ok()?;
                Some((c.number, rpm, c.label.clone()))
            })
            .collect()
    }
}

/// Label for a channel: the driver's fixed name on Thelio Io 2, otherwise
/// the hwmon fanN_label file.
fn read_label(hwmon: &Path, channel: u8, fixed_wiring: bool) -> String {
    if fixed_wiring {
        if let Some(label) = thelio_io_2_label(channel) {
            return label.to_string();
        }
    }
    fs::read_to_string(hwmon.join(format!("fan{}_label", channel)))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|_| format!("Fan {}", channel))
}

/// Latest GPU reading (temperature or usage).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuReading {
    /// Highest value across GPUs: °C for temperature, percent for usage
    Value(f32),
    /// The only GPUs are NVIDIA ones powered down by runtime power
    /// management; reading them would wake them up.
    Asleep,
    /// No GPU reports this value
    Unavailable,
}

impl GpuReading {
    pub fn value(self) -> Option<f32> {
        match self {
            GpuReading::Value(t) => Some(t),
            _ => None,
        }
    }
}

/// GPU temperature from amdgpu hwmon devices and NVIDIA's nvidia-smi,
/// the same sources system76-power reads.
pub struct GpuTemp {
    amdgpus: Vec<PathBuf>,
    nvidia: bool,
    /// NVIDIA PCI devices, to check whether they are powered down.
    nvidia_devices: Vec<PathBuf>,
    /// When the sources were last looked for
    scanned_at: Instant,
    /// After an nvidia-smi failure, it isn't called again until this time.
    nvidia_paused_until: Option<Instant>,
}

impl GpuTemp {
    pub fn discover() -> Self {
        let gpu = Self::scan();
        gpu.log_sources();
        gpu
    }

    fn scan() -> Self {
        let amdgpus: Vec<PathBuf> = fs::read_dir("/sys/class/hwmon")
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        fs::read_to_string(p.join("name"))
                            .map(|n| n.trim() == "amdgpu")
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let nvidia = Path::new("/proc/driver/nvidia/gpus").exists();
        let nvidia_devices: Vec<PathBuf> = fs::read_dir("/sys/bus/pci/devices")
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let read = |f: &str| fs::read_to_string(p.join(f)).unwrap_or_default();
                        // NVIDIA vendor id, display controller class
                        read("vendor").trim() == "0x10de" && read("class").starts_with("0x03")
                    })
                    .collect()
            })
            .unwrap_or_default();

        Self {
            amdgpus,
            nvidia,
            nvidia_devices,
            scanned_at: Instant::now(),
            nvidia_paused_until: None,
        }
    }

    fn log_sources(&self) {
        info!(
            "GPU temperature sources: {} amdgpu, nvidia-smi {}",
            self.amdgpus.len(),
            if self.nvidia {
                "enabled"
            } else {
                "not present"
            }
        );
    }

    /// Look for sources again if none were found, every GPU_RESCAN_INTERVAL.
    fn rescan_if_missing(&mut self) {
        if !self.amdgpus.is_empty()
            || self.nvidia
            || self.scanned_at.elapsed() < GPU_RESCAN_INTERVAL
        {
            return;
        }
        *self = Self::scan();
        if !self.amdgpus.is_empty() || self.nvidia {
            self.log_sources();
        }
    }

    fn nvidia_paused(&self) -> bool {
        self.nvidia_paused_until
            .is_some_and(|until| Instant::now() < until)
    }

    /// True when every NVIDIA GPU is runtime-suspended (e.g. the discrete GPU
    /// of a hybrid graphics laptop while idle).
    fn nvidia_asleep(&self) -> bool {
        !self.nvidia_devices.is_empty()
            && self.nvidia_devices.iter().all(|d| {
                fs::read_to_string(d.join("power/runtime_status"))
                    .map(|s| s.trim() == "suspended")
                    .unwrap_or(false)
            })
    }

    /// Hottest GPU temperature.
    pub fn read(&mut self) -> GpuReading {
        self.rescan_if_missing();
        let amd = self
            .amdgpus
            .iter()
            .filter_map(|p| fs::read_to_string(p.join("temp1_input")).ok())
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .map(|milli| milli as f32 / 1000.0);

        let mut asleep = false;
        let nvidia = if !self.nvidia {
            Vec::new()
        } else if self.nvidia_asleep() {
            // nvidia-smi would wake the GPU every second and drain the battery.
            asleep = true;
            Vec::new()
        } else if self.nvidia_paused() {
            Vec::new()
        } else {
            match nvidia_temperatures() {
                Ok(temps) => temps,
                Err(e) => {
                    // Back off rather than calling a failing nvidia-smi every
                    // second, but retry so a driver reload doesn't disable it.
                    warn!(
                        "Failed to read NVIDIA GPU temperature, retrying in {}s: {}",
                        NVIDIA_RETRY_INTERVAL.as_secs(),
                        e
                    );
                    self.nvidia_paused_until = Some(Instant::now() + NVIDIA_RETRY_INTERVAL);
                    Vec::new()
                }
            }
        };

        match amd.chain(nvidia).reduce(f32::max) {
            Some(t) => GpuReading::Value(t),
            None if asleep => GpuReading::Asleep,
            None => GpuReading::Unavailable,
        }
    }

    /// Busiest GPU's usage in percent. Intel integrated GPUs don't report
    /// usage without root-only tools, so they aren't included.
    pub fn read_usage(&mut self) -> GpuReading {
        self.rescan_if_missing();
        let amd = self
            .amdgpus
            .iter()
            .filter_map(|p| fs::read_to_string(p.join("device/gpu_busy_percent")).ok())
            .filter_map(|s| s.trim().parse::<f32>().ok());

        let mut asleep = false;
        let nvidia = if !self.nvidia {
            Vec::new()
        } else if self.nvidia_asleep() {
            asleep = true;
            Vec::new()
        } else if self.nvidia_paused() {
            Vec::new()
        } else {
            nvidia_query("utilization.gpu").unwrap_or_else(|e| {
                debug!("Failed to read NVIDIA GPU usage: {}", e);
                Vec::new()
            })
        };

        match amd.chain(nvidia).reduce(f32::max) {
            Some(u) => GpuReading::Value(u),
            None if asleep => GpuReading::Asleep,
            None => GpuReading::Unavailable,
        }
    }
}

/// Temperatures in °C of every NVIDIA GPU, via nvidia-smi.
fn nvidia_temperatures() -> std::io::Result<Vec<f32>> {
    nvidia_query("temperature.gpu")
}

/// One numeric nvidia-smi query field for every NVIDIA GPU.
fn nvidia_query(field: &str) -> std::io::Result<Vec<f32>> {
    let output = Command::new("nvidia-smi")
        .arg(format!("--query-gpu={}", field))
        .arg("--format=csv,noheader,nounits")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "nvidia-smi exited with {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<f32>().ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(channels: Vec<ChannelAssignment>) -> FanCurveConfig {
        let mut config = FanCurveConfig::new_for_model("");
        let mut flat = FanCurve::new("Flat 50".to_string());
        flat.add_point(0, 5000);
        config.curves.push(flat);
        config.channels = channels;
        config
    }

    #[test]
    fn default_sources_follow_thelio_io_2_wiring() {
        let config = config_with(Vec::new());
        // Standard is active: 45°C -> 30%, 75°C -> 50%
        let duty = |ch| channel_duty(&config, 0, true, ch, Some(45.0), Some(75.0));
        assert_eq!(duty(1), Some(3000), "CPU fan follows CPU");
        assert_eq!(duty(2), Some(5000), "intake follows hottest");
        assert_eq!(duty(3), Some(5000), "GPU fan follows GPU");
        assert_eq!(duty(4), Some(5000), "aux follows hottest");
    }

    #[test]
    fn unknown_wiring_uses_hottest_everywhere() {
        let config = config_with(Vec::new());
        assert_eq!(
            channel_duty(&config, 0, false, 1, Some(45.0), Some(75.0)),
            Some(5000)
        );
    }

    #[test]
    fn assignment_overrides_source_and_curve() {
        let config = config_with(vec![ChannelAssignment {
            channel: 3,
            source: TempSource::Cpu,
            curve: Some("Flat 50".to_string()),
        }]);
        assert_eq!(
            channel_duty(&config, 0, true, 3, Some(20.0), Some(90.0)),
            Some(5000)
        );
    }

    #[test]
    fn missing_curve_falls_back_to_active() {
        let config = config_with(vec![ChannelAssignment {
            channel: 1,
            source: TempSource::Cpu,
            curve: Some("Deleted".to_string()),
        }]);
        assert_eq!(
            channel_duty(&config, 0, true, 1, Some(45.0), None),
            Some(3000)
        );
    }

    #[test]
    fn missing_sensor_falls_back_to_the_other() {
        assert_eq!(source_temp(TempSource::Gpu, Some(40.0), None), Some(40.0));
        assert_eq!(source_temp(TempSource::Cpu, None, Some(60.0)), Some(60.0));
        assert_eq!(source_temp(TempSource::Hottest, None, None), None);
    }

    #[test]
    fn config_without_channels_still_loads() {
        let json = r#"{"curves": [], "default_curve_index": null}"#;
        let config: FanCurveConfig = serde_json::from_str(json).unwrap();
        assert!(config.channels.is_empty());
    }
}
