# System76 Power Codebase Schema Guide

This document guides contributions to stay cohesive with the system76-power codebase. Follow these patterns and conventions when adding or changing code.

## Project Overview

- **Purpose**: Manage power profiles and switchable graphics on laptops; expose control via DBus and integrate with UPower/PowerProfiles.
- **Binary**: `system76-power` supports daemon and CLI client modes.
- **Key interfaces**:
  - DBus service: name `com.system76.PowerDaemon`, path `/com/system76/PowerDaemon`, interface `com.system76.PowerDaemon`.
  - Compatibility shims: `org.freedesktop.UPower.PowerProfiles` and `net.hadess.PowerProfiles`.

## Toolchain and Dependencies

- **Rust**: edition 2021; MSRV is `rust-version = 1.75.0`.
- **Async runtime**: `tokio` with `#[tokio::main(flavor = "current_thread")]` for binaries.
- **DBus**: `zbus` 3.x and `zbus_polkit` for authorization checks.
- **Logging**: `log` + `fern` for setup, level via CLI flags.
- **Errors**: `anyhow` for app-level, `thiserror` for library-like enums in modules.
- **Sys interfaces**: `sysfs-class` for sysfs, `intel-pstate`, `hidapi`, `inotify`.
- **Workspace**: local crate `zbus/` provides typed client proxies.

## Architecture and Module Boundaries

- `src/main.rs`: Dispatches to daemon or client based on `Args`.
- `src/lib.rs`: Module declarations and DBus constants.
- `src/daemon/`:
  - `mod.rs`: Daemon construction, DBus interface impls, signal handling, integration with UPower/Hadess, background loops (fans, hotplug, mux).
  - `profiles.rs`: Implement Battery, Balanced, Performance parameter sets.
- `src/client.rs`: CLI-to-DBus: creates system bus connection, uses `system76_power_zbus::PowerDaemonProxy` to call methods, prints human-readable output.
- `src/graphics.rs`: Detect GPUs, implement mode switching, power control, and on-disk config mutations.
- Other modules: `acpi_platform`, `cpufreq`, `charge_thresholds`, `hid_backlight`, `hotplug`, `kernel_parameters`, `module`, `pci`, `radeon`, `runtime_pm`, `snd`, `sys_devices`, `wifi`, `errors`.

## Conventions

- **Naming**:
  - Modules reflect functional areas (e.g., `graphics`, `daemon`, `charge_thresholds`).
  - Public functions and types use descriptive names; avoid abbreviations.
  - DBus-exposed methods on `System76Power` use lowerCamelCase in Rust, and map to PascalCase in XML only when required (current iface uses PascalCase method names in the XML/DBus).
- **Async**: Prefer `current_thread` runtime and avoid blocking in async code; offload blocking work to threads where necessary.
- **Error handling**:
  - Return `zbus::fdo::Error::Failed` from DBus methods using a small adapter like `zbus_error_from_display`.
  - In daemon internals, collect multiple errors when applying power profiles (see `profiles.rs` pattern with `catch!`).
- **Logging**: Use `log::{info,warn,debug,error}`; initialize via `logging::setup` with verbosity from CLI.
- **Policy**: Use `zbus_polkit` to gate privileged DBus methods (e.g., `SetChargeThresholds`). Only the daemon runs as root.
- **File paths**: When changing system state, write to the same well-known paths used here, e.g., `/etc/modprobe.d/system76-power.conf`, `/usr/share/X11/xorg.conf.d/11-nvidia-discrete.conf`, `/etc/prime-discrete`.
- **Environment switches**: Honor existing env toggles (e.g., `S76_POWER_PCI_RUNTIME_PM`).

## DBus Patterns

- Define methods/signals on `System76Power` using `#[zbus::dbus_interface(name = "com.system76.PowerDaemon")]` and friend types for other exposed interfaces.
- For each property or method added:
  - Implement on the server side in `src/daemon/mod.rs` under the appropriate interface impl.
  - If user-facing via CLI, add a matching client call through `system76_power_zbus` proxy.
  - Update `data/com.system76.PowerDaemon.xml` only if external consumers need the introspection XML; prefer deriving from server code when possible.
- Emit signals via `zbus::SignalContext` created from the registered connection; follow existing `power_profile_switch` and `hot_plug_detect` examples.

## CLI Patterns

- Use `clap` derive in `src/args.rs`:
  - Keep `Args` enum as the main entrypoint with subcommands (`Daemon`, `Profile`, `Graphics`, `ChargeThresholds`).
  - For subcommand families (like graphics), create a nested enum `GraphicsArgs` with variants and validators.
- In `src/client.rs`:
  - Create a system bus connection, instantiate the `PowerDaemonProxy`, and dispatch based on `Args`.
  - Validate device capabilities (e.g., desktop vs laptop, switchable graphics) before calling DBus.
  - Map `zbus::Error` via a thin adapter to `anyhow` and print human-readable output.

## Power Profiles Pattern

- Implement per-profile functions in `daemon/profiles.rs` that accept `&mut Vec<ProfileError>` and a `set_brightness` boolean.
- Steps typically include:
  - ACPI platform profile selection if supported.
  - Kernel parameters: dirty ratios, laptop mode, runtime PM (guarded by `pci_runtime_pm_support()`).
  - Device tuning: Radeon profiles, SCSI/SATA link PM.
  - CPU tuning: `cpufreq::set(Profile, percent)`, `intel_pstate` values.
  - Backlights: screen and keyboard thresholds for Battery/Balanced.
  - Optional model-specific tweaks via `ModelProfiles`.
- Apply profiles via `PowerDaemon::apply_profile`, emitting signals and aggregating errors.

## Graphics Mode Pattern

- Modes: `Integrated`, `Hybrid`, `Discrete`, `Compute` (`GraphicsMode`).
- Source of truth:
  - PRIME switch file `/etc/prime-discrete` values: `off` (compute), `on` (discrete), `on-demand` (hybrid).
  - Loaded kernel modules to determine integrated.
- To switch modes (`Graphics::set_vendor`):
  - Write PRIME file, write `/etc/modprobe.d/system76-power.conf` with proper options/blacklists.
  - Configure S0ix/S3 nvidia options and toggle `nvidia-{hibernate,resume,suspend}.service` accordingly.
  - Manage Xorg conf for discrete mode; remove when not needed.
  - Enable/disable `nvidia-fallback.service`.
  - Run `update-initramfs -u`.
- Power control:
  - `get_power` checks device presence; `set_power(true)` rescans and sets `/sys/.../power/control`; `set_power(false)` unbinds and removes NVIDIA PCI functions.
  - `auto_power` enables power unless integrated mode without runtimepm.

## Adding New Functionality (Recipes)

### Add a new DBus method:
1. Implement on `System76Power` (or appropriate shim interface) in `src/daemon/mod.rs`.
2. Return `zbus::fdo::Result<T>` and convert errors via `zbus_error_from_display`.
3. If privileged, add a polkit check mirroring `set_charge_thresholds`.
4. Add a matching client path in `src/client.rs` via the zbus proxy and CLI arg in `src/args.rs`.
5. Update XML in `data/` if needed for external introspection consumers.

### Add a new CLI command:
1. Extend `Args` (and nested enums as needed) in `src/args.rs` using `clap` derive.
2. Handle dispatch in `src/client.rs`, validating device assumptions and printing clear output.

### Add a new power profile tweak:
1. Modify the corresponding function in `src/daemon/profiles.rs` and reuse the `catch!` macro to aggregate errors.
2. Avoid blocking I/O in async contexts; keep heavy I/O in synchronous profile code (called from DBus handler behind a mutex).

## Testing and Safety

- Build: `cargo build --release`.
- Runtime:
  - Daemon must run as root: `sudo ./target/release/system76-power daemon`.
  - Client commands should be safe to run unprivileged but expect DBus failures if daemon is missing.
- Risky operations:
  - Unbinding/removing PCI devices, updating initramfs, writing system config files. Ensure feature gating and thorough logging.
  - Check desktop vs laptop and switchable graphics before destructive actions.

## Style and Formatting

- Match existing formatting and module organization.
- Prefer descriptive names and early returns; handle errors explicitly.
- Do not introduce deep nesting; keep functions cohesive.
- Avoid comments for trivialities; add short doc comments for complex logic and rationale.

## When Adding Dependencies

- Prefer existing crates already used in the project (zbus, tokio, anyhow, thiserror).
- Keep features minimal; avoid default features for zbus unless required (maintain current feature flags).

## Client Crate Usage (`zbus/`)

- Prefer using `system76_power_zbus` generated proxy types in client code for DBus calls.
- If adding new DBus methods, regenerate or extend the local zbus proxy crate consistently.

---

Following this schema will keep changes aligned with the project's architecture, safety constraints, and UX.
