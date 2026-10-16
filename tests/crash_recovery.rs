//! The daemon crashing: a stand-in daemon holds fake laptop fans in manual
//! control and is killed with SIGKILL, then the ExecStopPost command from the
//! real service file runs the way systemd would. The fans must end up back on
//! automatic, and nothing else may be touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const SERVICE_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/fan-curve-daemon.service");
const INSTALLED_BINARY: &str = "/usr/local/bin/fan-curve-app";

/// A temporary /sys/class/hwmon with fake devices, removed afterwards.
struct FakeHwmon {
    root: PathBuf,
}

impl FakeHwmon {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!("fan_crash_{}_{}", test, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// A hwmon device with pwmN files and pwmN_enable files for `enables`.
    fn device(&self, dir: &str, name: &str, pwms: &[u8], enables: &[u8]) -> PathBuf {
        let dev = self.root.join(dir);
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("name"), format!("{}\n", name)).unwrap();
        for n in pwms {
            fs::write(dev.join(format!("pwm{}", n)), "0").unwrap();
        }
        for n in enables {
            fs::write(dev.join(format!("pwm{}_enable", n)), "2").unwrap();
        }
        dev
    }
}

impl Drop for FakeHwmon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_string()
}

/// Stand-in for the daemon on a laptop: every 50ms it switches `dev`'s fans
/// to manual control at a fixed test speed, like the daemon's control loop.
fn start_daemon(dev: &Path) -> Child {
    let script = format!(
        "while true; do \
            echo 1 > '{dev}/pwm1_enable'; echo 1 > '{dev}/pwm2_enable'; \
            echo 200 > '{dev}/pwm1'; echo 200 > '{dev}/pwm2'; \
            sleep 0.05; \
        done",
        dev = dev.display()
    );
    Command::new("sh").arg("-c").arg(script).spawn().unwrap()
}

/// Wait until `cond` holds, up to 5 seconds.
fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {}", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The service's ExecStopPost command, pointed at this build and `hwmon`.
fn exec_stop_post(hwmon: &Path) -> Command {
    let unit = fs::read_to_string(SERVICE_FILE).unwrap();
    let line = unit
        .lines()
        .find_map(|l| l.strip_prefix("ExecStopPost="))
        .expect("the service must hand the fans back after it stops");
    // "-": a failing recovery step must not mark the unit as failed
    let line = line
        .strip_prefix('-')
        .expect("ExecStopPost must be prefixed with '-'");
    let mut words = line.split_whitespace();
    assert_eq!(words.next(), Some(INSTALLED_BINARY));
    let mut command = Command::new(env!("CARGO_BIN_EXE_fan-curve-app"));
    command.args(words).arg("--hwmon-root").arg(hwmon);
    command
}

#[test]
fn killed_daemon_leaves_laptop_fans_on_automatic() {
    let hwmon = FakeHwmon::new("laptop");
    let laptop = hwmon.device("hwmon3", "system76_acpi", &[1, 2], &[1, 2]);

    // The daemon takes the fans and holds them at a test speed
    let mut daemon = start_daemon(&laptop);
    wait_for("the daemon to take the fans", || {
        read(&laptop.join("pwm1_enable")) == "1" && read(&laptop.join("pwm1")) == "200"
    });

    // It crashes: SIGKILL, so none of its own cleanup runs
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    assert_eq!(
        read(&laptop.join("pwm1_enable")),
        "1",
        "fans stuck in manual"
    );

    // systemd runs ExecStopPost after any stop
    let output = exec_stop_post(&hwmon.root).output().unwrap();
    assert!(output.status.success());

    for n in [1, 2] {
        assert_eq!(
            read(&laptop.join(format!("pwm{}_enable", n))),
            "2",
            "pwm{} is back on automatic",
            n
        );
    }
    // Once the stand-in is dead nothing writes the fans again
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(read(&laptop.join("pwm1_enable")), "2");
}

#[test]
fn recovery_after_a_crash_only_touches_system76_fans() {
    let hwmon = FakeHwmon::new("others");
    let laptop = hwmon.device("hwmon3", "system76_acpi", &[1], &[1]);
    // A Thelio Io 2 has no enable files; another fan controller is left in
    // manual control by its own software
    let thelio = hwmon.device("hwmon5", "system76_thelio_io", &[1, 2, 3, 4], &[]);
    let other = hwmon.device("hwmon6", "nct6798", &[1], &[1]);
    fs::write(other.join("pwm1_enable"), "1").unwrap();

    let mut daemon = start_daemon(&laptop);
    wait_for("the daemon to take the fans", || {
        read(&laptop.join("pwm1_enable")) == "1"
    });
    daemon.kill().unwrap();
    daemon.wait().unwrap();

    let output = exec_stop_post(&hwmon.root).output().unwrap();
    assert!(output.status.success());

    assert_eq!(read(&laptop.join("pwm1_enable")), "2");
    assert_eq!(
        read(&other.join("pwm1_enable")),
        "1",
        "not a System76 device"
    );
    assert!(!thelio.join("pwm1_enable").exists(), "nothing created");
    assert_eq!(read(&laptop.join("pwm1")), "200", "speeds aren't rewritten");
}

#[test]
fn recovery_with_no_system76_fans_succeeds() {
    // e.g. the board was unplugged: the stop step must still succeed
    let hwmon = FakeHwmon::new("none");
    hwmon.device("hwmon0", "k10temp", &[], &[]);
    let output = exec_stop_post(&hwmon.root).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("No System76 fan controls"));
}
