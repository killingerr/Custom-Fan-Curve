// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use log::{debug, error};
use std::{
    collections::HashMap,
    fmt::Display,
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
    path::Path,
};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::Mutex,
    time::sleep,
};
use zbus::Interface;

use crate::{
    charge_thresholds::{get_charge_profiles, get_charge_thresholds, set_charge_thresholds},
    errors::ProfileError,
    fan::{FanDaemon, FanCurveConfig},
    graphics::{Graphics, GraphicsMode},
    hid_backlight,
    hotplug::{mux, Detect, HotPlugDetect},
    kernel_parameters::{KernelParameter, NmiWatchdog},
    runtime_pm::runtime_pm_quirks,
    DBUS_NAME, DBUS_PATH,
};

mod profiles;
use self::profiles::{balanced, battery, performance};

use system76_power_zbus::ChargeProfile;

const THRESHOLD_POLICY: &str = "com.system76.powerdaemon.set-charge-thresholds";
const NET_HADESS_POWER_PROFILES_DBUS_NAME: &str = "net.hadess.PowerProfiles";
const NET_HADESS_POWER_PROFILES_DBUS_PATH: &str = "/net/hadess/PowerProfiles";
const POWER_PROFILES_DBUS_NAME: &str = "org.freedesktop.UPower.PowerProfiles";
const POWER_PROFILES_DBUS_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

static CONTINUE: AtomicBool = AtomicBool::new(true);

async fn signal_handling() {
    let mut int = signal(SignalKind::interrupt()).unwrap();
    let mut hup = signal(SignalKind::hangup()).unwrap();
    let mut term = signal(SignalKind::terminate()).unwrap();

    let sig = tokio::select! {
        _ = int.recv() => "SIGINT",
        _ = hup.recv() => "SIGHUP",
        _ = term.recv() => "SIGTERM"
    };

    log::info!("caught signal: {}", sig);
    CONTINUE.store(false, Ordering::SeqCst);
}

// Disabled by default because some systems have quirky ACPI tables that fail to resume from
// suspension.
static PCI_RUNTIME_PM: AtomicBool = AtomicBool::new(false);

// TODO: Whitelist system76 hardware that's known to work with this setting.
pub(crate) fn pci_runtime_pm_support() -> bool { PCI_RUNTIME_PM.load(Ordering::SeqCst) }

struct PowerDaemon {
    initial_set:    bool,
    graphics:       Graphics,
    power_profile:  String,
    profile_errors: Vec<ProfileError>,
    held_profiles:  Vec<(u32, &'static str, String, String)>,
    profile_ids:    u32,
    connections:    Option<(zbus::Connection, zbus::Connection, zbus::Connection)>,
    fan_daemon:     FanDaemon,
}

impl PowerDaemon {
    fn new() -> anyhow::Result<Self> {
        let graphics = Graphics::new()?;
        let nvidia_exists = !graphics.nvidia.is_empty();
        let fan_daemon = FanDaemon::new(nvidia_exists);

        Ok(Self {
            initial_set: false,
            graphics,
            power_profile: String::new(),
            profile_errors: Vec::new(),
            held_profiles: Vec::new(),
            profile_ids: 0,
            connections: None,
            fan_daemon,
        })
    }

    async fn apply_profile(
        &mut self,
        context: &zbus::SignalContext<'_>,
        func: fn(&mut Vec<ProfileError>, bool),
        name: &str,
    ) -> Result<(), String> {
        if self.power_profile == name {
            log::info!("profile was already set");
            return Ok(());
        }

        let _res = System76Power::power_profile_switch(context, name).await;

        func(&mut self.profile_errors, self.initial_set);

        self.power_profile = name.into();

        if self.profile_errors.is_empty() {
            Ok(())
        } else {
            let mut error_message = String::from("Errors found when setting profile:");
            for error in self.profile_errors.drain(..) {
                error_message = format!("{}\n    - {}", error_message, error);
            }

            Err(error_message)
        }
    }
    
    fn load_fan_curve_from_file(&self, path: &Path) -> Result<FanCurveConfig, Box<dyn std::error::Error>> {
        let file_content = fs::read_to_string(path)?;
        let config: FanCurveConfig = serde_json::from_str(&file_content)?;
        Ok(config)
    }
    
    pub fn load_all_fan_curves(&self) -> zbus::fdo::Result<Vec<(String, Vec<(u8, u8)>)>> {
    let mut curves = Vec::new();
    let directories = [
        "/etc/system76-power/fan_curves",
        "/var/lib/system76-power/fan_curves",
    ];

    for dir in &directories {
        debug!("Scanning directory: {}", dir);
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_file() {
                        if let Some(file_name) = entry.file_name().to_str() {
                            if file_name.ends_with(".json") {
                                debug!("Found JSON file: {}", file_name);
                                match self.load_fan_curve_from_file(&entry.path()) {
                                    Ok(curve) => {
                                        debug!("Loaded curve: {} with {} points", curve.name, curve.points.len());
                                        if curves.iter().any(|(name, _)| name == &curve.name) {
                                            debug!("Skipping duplicate curve: {}", curve.name);
                                        } else {
                                            curves.push((
                                                curve.name.clone(),
                                                curve.points
                                            ));
                                            debug!("Added curve: {} to the list", curve.name);
                                        }
                                    },
                                    Err(e) => {
                                        error!("Failed to load curve from file {}: {}", file_name, e);
                                    }
                                }
                            } else {
                                debug!("Skipping non-JSON file: {}", file_name);
                            }
                        }
                    }
                }
            }
        } else {
            error!("Failed to read directory: {}", dir);
        }
    }

    debug!("Total curves loaded: {}", curves.len());
    for (index, (name, points)) in curves.iter().enumerate() {
        debug!("Curve {}: {} with {} points", index + 1, name, points.len());
    }
    Ok(curves)
}

}

#[derive(Clone)]
struct System76Power(Arc<Mutex<PowerDaemon>>);
impl System76Power {
    pub async fn emit_active_profile_changed(&self) {
        let (upp_connection, hadess_connection, profile) = {
            let this = self.0.lock().await;
            let Some((_, upp, hadess)) = this.connections.clone() else { return };

            let profile = system76_profile_to_upp_str(&this.power_profile);
            (upp, hadess, profile)
        };

        let value = zvariant::Value::Str(zvariant::Str::from(profile));
        let changed = HashMap::from_iter(std::iter::once(("ActiveProfile", &value)));
        let invalidated = &[];

        if let Ok(context) = zbus::SignalContext::new(&upp_connection, POWER_PROFILES_DBUS_PATH) {
            let _res = zbus::fdo::Properties::properties_changed(
                &context,
                UPowerPowerProfiles::name(),
                &changed,
                invalidated,
            )
            .await;
        }

        if let Ok(context) =
            zbus::SignalContext::new(&hadess_connection, NET_HADESS_POWER_PROFILES_DBUS_PATH)
        {
            let _res = zbus::fdo::Properties::properties_changed(
                &context,
                NetHadessPowerProfiles::name(),
                &changed,
                invalidated,
            )
            .await;
        }
    }
}

#[zbus::dbus_interface(name = "com.system76.PowerDaemon")]
impl System76Power {
    async fn battery(
        &mut self,
        #[zbus(signal_context)] context: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = self
            .0
            .lock()
            .await
            .apply_profile(&context, battery, "Battery")
            .await
            .map_err(zbus_error_from_display);

        if result.is_ok() {
            self.emit_active_profile_changed().await
        }

        result
    }

    async fn balanced(
        &mut self,
        #[zbus(signal_context)] context: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = self
            .0
            .lock()
            .await
            .apply_profile(&context, balanced, "Balanced")
            .await
            .map_err(zbus_error_from_display);

        if result.is_ok() {
            self.emit_active_profile_changed().await
        }

        result
    }

    async fn performance(
        &mut self,
        #[zbus(signal_context)] context: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = self
            .0
            .lock()
            .await
            .apply_profile(&context, performance, "Performance")
            .await
            .map_err(zbus_error_from_display);

        if result.is_ok() {
            self.emit_active_profile_changed().await
        }

        result
    }

    #[dbus_interface(out_args("profile"))]
    async fn get_profile(&self) -> zbus::fdo::Result<String> {
        Ok(self.0.lock().await.power_profile.clone())
    }

    #[dbus_interface(out_args("required"))]
    async fn get_external_displays_require_dgpu(&mut self) -> zbus::fdo::Result<bool> {
        self.0
            .lock()
            .await
            .graphics
            .get_external_displays_require_dgpu()
            .map_err(zbus_error_from_display)
    }

    #[dbus_interface(out_args("vendor"))]
    async fn get_default_graphics(&self) -> zbus::fdo::Result<String> {
        self.0
            .lock()
            .await
            .graphics
            .get_default_graphics()
            .map_err(zbus_error_from_display)
            .map(|mode| <&'static str>::from(mode).to_owned())
    }

    #[dbus_interface(out_args("vendor"))]
    async fn get_graphics(&self) -> zbus::fdo::Result<String> {
        self.0
            .lock()
            .await
            .graphics
            .get_vendor()
            .map_err(zbus_error_from_display)
            .map(|mode| <&'static str>::from(mode).to_owned())
    }
     
    async fn set_graphics(&mut self, vendor: &str) -> zbus::fdo::Result<()> {
        self.0
            .lock()
            .await
            .graphics
            .set_vendor(GraphicsMode::from(vendor))
            .map_err(zbus_error_from_display)
    }

    #[dbus_interface(out_args("desktop"))]
    async fn get_desktop(&mut self) -> zbus::fdo::Result<bool> {
        Ok(self.0.lock().await.graphics.is_desktop())
    }

    #[dbus_interface(out_args("switchable"))]
    async fn get_switchable(&mut self) -> zbus::fdo::Result<bool> {
        Ok(self.0.lock().await.graphics.can_switch())
    }

    #[dbus_interface(out_args("power"))]
    async fn get_graphics_power(&mut self) -> zbus::fdo::Result<bool> {
        self.0.lock().await.graphics.get_power().map_err(zbus_error_from_display)
    }

    async fn set_graphics_power(&mut self, power: bool) -> zbus::fdo::Result<()> {
        self.0.lock().await.graphics.set_power(power).map_err(zbus_error_from_display)
    }

    async fn auto_graphics_power(&mut self) -> zbus::fdo::Result<()> {
        self.0.lock().await.graphics.auto_power().map_err(zbus_error_from_display)
    }

    #[dbus_interface(out_args("start", "end"))]
    async fn get_charge_thresholds(&mut self) -> zbus::fdo::Result<(u8, u8)> {
        get_charge_thresholds().map_err(zbus_error_from_display)
    }

    async fn set_charge_thresholds(&mut self, thresholds: (u8, u8)) -> zbus::fdo::Result<()> {
        let connection = zbus::Connection::system().await?;
        let polkit = zbus_polkit::policykit1::AuthorityProxy::new(&connection)
            .await
            .context("could not connect to polkit authority daemon")
            .map_err(zbus_error_from_display)?;

        let pid = std::process::id();

        let permitted = if pid == 0 {
            true
        } else {
            let subject = zbus_polkit::policykit1::Subject::new_for_owner(pid, None, None)
                .context("could not create policykit1 subject")
                .map_err(zbus_error_from_display)?;

            polkit
                .check_authorization(
                    &subject,
                    THRESHOLD_POLICY,
                    &std::collections::HashMap::new(),
                    Default::default(),
                    "",
                )
                .await
                .context("could not check policykit authorization")
                .map_err(zbus_error_from_display)?
                .is_authorized
        };

        if permitted {
            set_charge_thresholds(thresholds).map_err(zbus_error_from_display)
        } else {
            Err(zbus_error_from_display("Operation not permitted by Polkit"))
        }
    }

    #[dbus_interface(out_args("profiles"))]
    async fn get_charge_profiles(&mut self) -> zbus::fdo::Result<Vec<ChargeProfile>> {
        Ok(get_charge_profiles())
    }
    
    
    async fn set_fan_curve(&mut self, curve: Vec<(u8, u8)>) -> zbus::fdo::Result<()> {
        log::info!("SetFanCurve method registered");
        log::info!("SetFanCurve called with {} points", curve.len());
        let mut daemon = self.0.lock().await;
        daemon.fan_daemon.set_fan_curve(None, curve, false)
            .map_err(zbus_error_from_display)
    }
    
    async fn set_fan_curve_persistent(&mut self, name: String, curve: Vec<(u8, u8)>) -> zbus::fdo::Result<()> {
        let mut daemon = self.0.lock().await;
    
        let file_name = format!("{}.json", name.replace(" ", "_").to_lowercase());
        let etc_path = Path::new("/etc/system76-power/fan_curves").join(&file_name);
        let var_path = Path::new("/var/lib/system76-power/fan_curves").join(&file_name);
    
        daemon.fan_daemon.set_fan_curve(Some(name.clone()), curve.clone(), false)
            .map_err(|e| zbus::fdo::Error::Failed(e))?;
    
        // Create directories if they don't exist
        if let Err(e) = fs::create_dir_all(etc_path.parent().unwrap()) {
            return Err(zbus::fdo::Error::Failed(format!("Failed to create directory: {}", e)));
        }
        if let Err(e) = fs::create_dir_all(var_path.parent().unwrap()) {
            return Err(zbus::fdo::Error::Failed(format!("Failed to create directory: {}", e)));
        }
    
        // Save to both locations
        if let Err(e) = daemon.fan_daemon.save_fan_curve(&etc_path, name.clone()) {
            return Err(zbus::fdo::Error::Failed(format!("Failed to save fan curve to /etc: {}", e)));
        }
        if let Err(e) = daemon.fan_daemon.save_fan_curve(&var_path, name.clone()) {
            return Err(zbus::fdo::Error::Failed(format!("Failed to save fan curve to /var/lib: {}", e)));
        }
        
        // Set as default
        let default_path = Path::new("/etc/system76-power/fan_curves/default.json");
        if let Err(e) = daemon.fan_daemon.save_fan_curve(default_path, name) {
            return Err(zbus::fdo::Error::Failed(format!("Failed to save default fan curve: {}", e)));
        }
    
        Ok(())
            
    }
    
    #[dbus_interface(name = "LoadAllFanCurves")]
    async fn load_all_fan_curves(&self) -> zbus::fdo::Result<Vec<(String, Vec<(u8, u8)>)>> {
        let daemon = self.0.lock().await;
        daemon.load_all_fan_curves()
    }
    
    #[dbus_interface(out_args("curve"))]
    async fn get_fan_curve(&self) -> zbus::fdo::Result<Vec<(u8, u8)>> {
        log::info!("GetFanCurve method registered");
        log::info!("GetFanCurve called");
        let daemon = self.0.lock().await;
        Ok(daemon.fan_daemon.get_fan_curve())
    }

    #[dbus_interface(signal)]
    async fn hot_plug_detect(context: &zbus::SignalContext<'_>, port: u64) -> zbus::Result<()>;

    #[dbus_interface(signal)]
    async fn power_profile_switch(
        context: &zbus::SignalContext<'_>,
        profile: &str,
    ) -> zbus::Result<()>;
}

struct UPowerPowerProfiles(Arc<Mutex<PowerDaemon>>);

impl UPowerPowerProfiles {
    pub async fn apply_held_profile(&mut self) {
        let mut set_profile = "balanced";

        for (_, profile, ..) in &self.0.lock().await.held_profiles {
            match *profile {
                "power-saver" => {
                    set_profile = "power-saver";
                    break;
                }
                "performance" => set_profile = "performance",
                _ => (),
            }
        }

        self.set_active_profile(set_profile).await;
    }
}

#[zbus::dbus_interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl UPowerPowerProfiles {
    #[dbus_interface(out_args("cookie"))]
    async fn hold_profile(
        &mut self,
        profile: &str,
        reason: &str,
        application_id: &str,
    ) -> zbus::fdo::Result<u32> {
        let mut this = self.0.lock().await;
        let id = this.profile_ids;

        let profile_static = match profile {
            "power-saver" => "power-saver",
            "balanced" => "balanced",
            "performance" => "performance",
            _ => return Err(zbus::fdo::Error::Failed(String::from("unknown power profile"))),
        };

        this.profile_ids += 1;
        this.held_profiles.push((id, profile_static, reason.into(), application_id.into()));
        drop(this);

        self.apply_held_profile().await;

        Ok(id)
    }

    async fn release_profile(&mut self, cookie: u32) {
        let mut this = self.0.lock().await;

        if let Some(pos) = this.held_profiles.iter().position(|(id, ..)| *id == cookie) {
            this.held_profiles.swap_remove(pos);
            drop(this);

            self.apply_held_profile().await;

            let this = self.0.lock().await;
            let Some((_, ref connection, _)) = this.connections else {
                return;
            };

            if let Ok(context) = zbus::SignalContext::new(connection, POWER_PROFILES_DBUS_PATH) {
                let _res = Self::profile_released(&context, cookie);
            }
        }
    }

    #[dbus_interface(signal)]
    async fn profile_released(context: &zbus::SignalContext<'_>, cookie: u32) -> zbus::Result<()>;

    #[dbus_interface(property)]
    async fn active_profile(&self) -> &str {
        system76_profile_to_upp_str(self.0.lock().await.power_profile.as_str())
    }

    #[dbus_interface(property)]
    async fn set_active_profile(&mut self, profile: &str) {
        let (func, profile): (fn(&mut Vec<ProfileError>, bool), &'static str) = match profile {
            "power-saver" => (battery, "Battery"),
            "balanced" => (balanced, "Balanced"),
            "performance" => (performance, "Performance"),
            _ => return,
        };

        let mut this = self.0.lock().await;
        let Some((ref connection, ..)) = this.connections else {
            return;
        };

        if let Ok(context) = zbus::SignalContext::new(connection, DBUS_PATH) {
            let _res =
                this.apply_profile(&context, func, profile).await.map_err(zbus_error_from_display);
        }
    }

    #[dbus_interface(property)]
    async fn profiles(&self) -> Vec<HashMap<&'static str, zvariant::Value>> {
        vec![
            {
                let mut map = HashMap::new();
                map.insert("Profile", zvariant::Value::Str(zvariant::Str::from("balanced")));
                map
            },
            {
                let mut map = HashMap::new();
                map.insert("Profile", zvariant::Value::Str(zvariant::Str::from("performance")));
                map
            },
            {
                let mut map = HashMap::new();
                map.insert("Profile", zvariant::Value::Str(zvariant::Str::from("power-saver")));
                map
            },
        ]
    }

    #[dbus_interface(property)]
    async fn performance_degraded(&self) -> &str { "" }

    #[dbus_interface(property)]
    async fn performance_inhibited(&self) -> &str { "" }

    #[dbus_interface(property)]
    async fn active_profile_holds(&self) -> Vec<HashMap<String, zvariant::Value>> { Vec::new() }

    #[dbus_interface(property)]
    async fn actions(&self) -> Vec<String> { vec![] }

    #[dbus_interface(property)]
    async fn version(&self) -> &str { "system76-power 1.2.0" }
}

pub struct NetHadessPowerProfiles(UPowerPowerProfiles);

#[zbus::dbus_interface(name = "net.hadess.PowerProfiles")]
impl NetHadessPowerProfiles {
    #[dbus_interface(property)]
    async fn active_profile(&self) -> &str { self.0.active_profile().await }

    #[dbus_interface(property)]
    async fn set_active_profile(&mut self, profile: &str) {
        self.0.set_active_profile(profile).await
    }

    #[dbus_interface(property)]
    async fn performance_inhibited(&self) -> &str { self.0.performance_inhibited().await }

    #[dbus_interface(property)]
    async fn profiles(&self) -> Vec<HashMap<&'static str, zvariant::Value>> {
        self.0.profiles().await
    }

    #[dbus_interface(property)]
    async fn actions(&self) -> Vec<String> { self.0.actions().await }
}

#[tokio::main(flavor = "current_thread")]
#[allow(clippy::too_many_lines)]
pub async fn daemon() -> anyhow::Result<()> {
    let signal_handling_fut = signal_handling();

    let pci_runtime_pm = std::env::var("S76_POWER_PCI_RUNTIME_PM").ok().map_or(false, |v| v == "1");

    PCI_RUNTIME_PM.store(pci_runtime_pm, Ordering::SeqCst);

    let daemon = PowerDaemon::new()?;

    let nvidia_exists = !daemon.graphics.nvidia.is_empty();

    NmiWatchdog.set(b"0");

    // Get the NVIDIA device ID before potentially removing it.
    let nvidia_device_id = if nvidia_exists {
        fs::read_to_string("/sys/bus/pci/devices/0000:01:00.0/device").ok()
    } else {
        None
    };

    let daemon = Arc::new(Mutex::new(daemon));
    let mut system76_daemon = System76Power(daemon.clone());
    
    log::info!("Registering com.system76.PowerDaemon DBus interface");
    

    match system76_daemon.auto_graphics_power().await {
        Ok(()) => (),
        Err(err) => {
            log::warn!("Failed to set automatic graphics power: {}", err);
        }
    }

    match runtime_pm_quirks() {
        Ok(()) => (),
        Err(err) => {
            log::warn!("Failed to set runtime power management quirks: {}", err);
        }
    }

    // Register DBus interface for org.freedesktop.UPower.PowerProfiles.
    // This is used by powerprofilesctl
    let upp_connection = zbus::ConnectionBuilder::system()
        .context("failed to create zbus connection builder")?
        .name(POWER_PROFILES_DBUS_NAME)
        .context("unable to register name")?
        .serve_at(POWER_PROFILES_DBUS_PATH, UPowerPowerProfiles(daemon.clone()))
        .context("unable to serve")?
        .build()
        .await
        .context("unable to create system service for org.freedesktop.UPower.PowerProfiles")?;

    // Register DBus interface for net.hadess.PowerProfiles.
    // This is used by gnome-shell
    let hadess_connection = zbus::ConnectionBuilder::system()
        .context("failed to create zbus connection builder")?
        .name(NET_HADESS_POWER_PROFILES_DBUS_NAME)
        .context("unable to register name")?
        .serve_at(
            NET_HADESS_POWER_PROFILES_DBUS_PATH,
            NetHadessPowerProfiles(UPowerPowerProfiles(daemon)),
        )
        .context("unable to serve")?
        .build()
        .await
        .context("unable to create system service for net.hadess.PowerProfiles")?;

    log::info!("Registering com.system76.PowerDaemon DBus interface");
    // Register DBus interface for com.system76.PowerDaemon.
    let connection = zbus::ConnectionBuilder::system()
        .context("failed to create zbus connection builder")?
        .name(DBUS_NAME)
        .context("unable to register name")?
        .serve_at(DBUS_PATH, system76_daemon.clone())
        .context("unable to serve")?
        .build()
        .await
        .context("unable to create system service for com.system76.PowerDaemon")?;
        
     log::info!("Registered com.system76.PowerDaemon DBus interface");

    system76_daemon.0.lock().await.connections =
        Some((connection.clone(), upp_connection, hadess_connection));

    let context = zbus::SignalContext::new(&connection, DBUS_PATH)
        .context("unable to create signal context")?;

    if let Err(why) = system76_daemon.balanced(context.clone()).await {
        log::warn!("Failed to set initial profile: {}", why);
    }

    system76_daemon.0.lock().await.initial_set = true;

    // Spawn hid backlight daemon
    let _hid_backlight = thread::spawn(hid_backlight::daemon);
    let mut fan_daemon = FanDaemon::new(nvidia_exists);
    let mut hpd_res = unsafe { HotPlugDetect::new(nvidia_device_id) };
    let mux_res = unsafe { mux::DisplayPortMux::new() };
    let mut hpd = || -> [bool; 4] {
        if let Ok(ref mut hpd) = hpd_res {
            unsafe { hpd.detect() }
        } else {
            [false; 4]
        }
    };

    let main_loop = async move {
        let mut last = hpd();

        while CONTINUE.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(1000)).await;

            fan_daemon.step();

            let hpd = hpd();
            for i in 0..hpd.len() {
                if hpd[i] != last[i] && hpd[i] {
                    log::info!("HotPlugDetect {}", i);
                    let _res = System76Power::hot_plug_detect(&context, i as u64).await;
                }
            }

            last = hpd;

            if let Ok(ref mux) = mux_res {
                unsafe {
                    mux.step();
                }
            }
        }
    };

    log::info!("Handling dbus requests");
    futures_lite::future::zip(signal_handling_fut, main_loop).await;

    log::info!("daemon exited from loop");
    Ok(())
}

fn system76_profile_to_upp_str(system76_profile: &str) -> &'static str {
    match system76_profile {
        "Battery" => "power-saver",
        "Balanced" => "balanced",
        "Performance" => "performance",
        _ => "unknown",
    }
}

fn zbus_error_from_display<E: Display>(why: E) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!("{}", why))
}
