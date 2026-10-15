# Fan Curve Control App

A modern GUI application for controlling fan curves on System76 desktops, built with Rust and egui.

## Features

- **Fan Curve Profiles** - Create, save and switch between fan curve profiles, and pick the one used after a restart
- **Per-Fan Control** - On Thelio desktops, sync all fans to one curve or give the CPU, intake, GPU and aux fans each their own curve and temperature source
- **Live Monitoring** - CPU and GPU temperatures and usage, and each fan's duty and speed, with N/A for ports that have no fan
- **System76 Integration** - Uses system76-power's built-in curves and takes over fan control from it while running
- **Overheat Protection** - Runs every fan at 100% from 90°C until temperatures drop below 80°C, with desktop notifications

## Quick Installation

### One-Command Install

```bash
curl -sSL https://raw.githubusercontent.com/killingerr/Custom-Fan-Curve/main/install.sh | bash
```

### Manual Installation

1. **Clone the repository:**
   ```bash
   git clone https://github.com/killingerr/Custom-Fan-Curve.git
   cd Custom-Fan-Curve
   ```

2. **Run the installation script:**
   ```bash
   ./install.sh --local
   ```
   `--local` builds and installs this checkout, including any changes you've
   made. Without it, the script downloads and installs the latest code from
   GitHub instead.

3. **Launch the application:**
   ```bash
   fan-curve --gui
   ```

## Requirements

- **Operating System:** Linux (tested on Ubuntu, Pop!_OS, Arch Linux)
- **Hardware:** System76 desktop
- **Dependencies:** Rust 1.75+, build tools, X11 libraries

The installation script will automatically install all required dependencies.

## Usage

### Command Line Interface

```bash
# Launch GUI
fan-curve --gui

# List available fan curves
fan-curve fan-curve list

# Apply a specific curve
fan-curve fan-curve set "Standard"

# Show help
fan-curve --help
```

### GUI Application

1. Launch the application from your applications menu or run `fan-curve --gui`
2. Select a profile in "Select Fan Curve"
3. Click "Apply Fan Curve" to use it until the next restart, or "Set as Default" to keep using it after restarts
4. To change a built-in profile, click "Save as New Profile" and edit the copy
5. Choose "Sync all fans" to drive every fan from the profile's points, or "Control each fan independently" to give each fan its own points and temperature source

## Configuration

The daemon (`fan-curve-daemon.service`) controls the fans and keeps its profiles in
`/etc/fan-curve-app/config.json`; the GUI changes them through it. On Thelio
desktops each fan port of the Thelio Io board is driven through the kernel's
`system76_thelio_io` driver.

### Default Curves

The built-in curves are the same as system76-power's, and the default is picked
by model the same way:

- **Standard** - Most systems
- **HEDT** - Thelio Major (r2 and later), Mega and Astra
- **Threadripper 2** - Thelio Major r1
- **Xeon** - Thelio Massive

Built-in curves can't be overwritten; use "Save as New Profile" to keep changes.

## Building from Source

If you prefer to build from source:

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/killingerr/Custom-Fan-Curve.git
cd Custom-Fan-Curve
cargo build --release

# Run
./target/release/fan-curve-app --gui
```

## Uninstallation

The installer creates an uninstall script that removes the daemon, the
application, its configuration and its desktop entries:

```bash
~/uninstall-fan-curve-app.sh
```

## Troubleshooting

### Permission Issues
If you encounter permission issues, make sure you're not running as root and that the installation script can use sudo when needed.

### Desktop Entry Not Appearing
After installation, you may need to log out and back in for the desktop entry to appear in your applications menu.

### Build Issues
If you encounter build issues, make sure all dependencies are installed:
```bash
# Ubuntu/Debian
sudo apt-get install build-essential pkg-config libssl-dev libx11-dev libxcb1-dev

# Arch Linux
sudo pacman -S base-devel openssl pkgconf libx11 libxcb
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Support

For issues and questions, please open an issue on the [GitHub repository](https://github.com/killingerr/Custom-Fan-Curve/issues).