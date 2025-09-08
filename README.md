# Fan Curve Control App

A modern GUI application for controlling CPU fan curves on System76 laptops, built with Rust and egui.

## Features

- 🎛️ **Interactive Fan Curve Editor** - Create custom fan curves with an intuitive GUI
- 🌡️ **Real-time Temperature Monitoring** - Monitor CPU temperatures and fan speeds
- 📊 **Multiple Curve Profiles** - Save and switch between different fan curve configurations
- 🔧 **System76 Integration** - Designed specifically for System76 laptops
- 🧩 **Optional Thelio IO Integration** - Experimental hook to the Thelio IO daemon
- 🚀 **High Performance** - Built with Rust for optimal performance and reliability

## Quick Installation

### One-Command Install

```bash
curl -sSL https://raw.githubusercontent.com/tonysystem76/Fan-Curve-App/main/install.sh | bash
```

### Manual Installation

1. **Clone the repository:**
   ```bash
   git clone https://github.com/tonysystem76/Fan-Curve-App.git
   cd Fan-Curve-App
   ```

2. **Run the installation script:**
   ```bash
   ./install.sh
   ```

3. **Launch the application:**
   ```bash
   fan-curve --gui
   ```

## Requirements

- **Operating System:** Linux (tested on Ubuntu, Pop!_OS, Arch Linux)
- **Hardware:** System76 laptop
- **Dependencies:** Rust 1.75+, build tools, X11 libraries

The installation script will automatically install all required dependencies.

## Usage

### Command Line Interface

```bash
# Launch GUI
fan-curve --gui

# List available fan curves
fan-curve list

# Apply a specific curve
fan-curve apply "Performance"

# Show help
fan-curve --help
```

### GUI Application

1. Launch the application from your applications menu or run `fan-curve --gui`
2. Select a fan curve from the dropdown menu
3. Click "Apply" to set the fan curve
4. Use "Edit" to modify existing curves or create new ones

## Configuration
### Thelio IO (Experimental)

Set an environment variable to enable Thelio IO integration. When enabled, the app will attempt to detect the Thelio IO DBus service and, if present, use it as a backend for chassis fan telemetry/control in future updates.

```bash
export FAN_APP_ENABLE_THELIO_IO=1
# Optional: override service name if needed
export FAN_APP_THELIO_IO_SERVICE="com.system76.ThelioIo"
```

Notes:
- If the service is not present, the application continues to function normally (no-op backend).
- Current implementation includes stubs for fan RPM/duty and temperature; concrete wiring can be added once the interface is finalized.


Configuration files are stored in `~/.fan_curve_app/config.json`. You can edit this file directly or use the GUI to modify settings.

### Default Curves

- **Standard** - Balanced performance and noise
- **Quiet** - Lower fan speeds for quieter operation
- **Performance** - Higher fan speeds for maximum cooling

## Building from Source

If you prefer to build from source:

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/tonysystem76/Fan-Curve-App.git
cd Fan-Curve-App
cargo build --release

# Run
./target/release/fan-curve-app --gui
```

## Uninstallation

To uninstall the application:

```bash
# Run the uninstall script
~/uninstall-fan-curve-app.sh

# Or manually remove:
sudo rm /usr/local/bin/fan-curve-app /usr/local/bin/fan-curve
rm -rf ~/.fan_curve_app
rm ~/.local/share/applications/fan-curve-app.desktop
rm ~/.local/share/icons/fan-curve-app.svg
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

For issues and questions, please open an issue on the [GitHub repository](https://github.com/tonysystem76/Fan-Curve-App/issues).