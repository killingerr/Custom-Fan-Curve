# Fan Curve Control App

A modern Rust-based application for managing CPU fan curves on System76 laptops with both GUI and command-line interfaces.

## Features

- 🎛️ **Interactive GUI**: Easy-to-use graphical interface for fan curve management
- 📊 **Real-time Monitoring**: Live temperature and fan speed monitoring during tests
- 🔧 **CLI Interface**: Command-line tools for automation and scripting
- 🌡️ **Temperature Interpolation**: Smooth fan curve transitions using linear interpolation
- 💾 **Configuration Management**: Save and load custom fan curve profiles
- 🧪 **Testing Mode**: Test fan curves with real-time data logging

## Quick Start

### Prerequisites

- Rust 1.75.0 or later
- Linux (tested on System76 laptops)

### Installation

1. **Clone the repository:**
   ```bash
   git clone https://github.com/yourusername/fan-curve-app.git
   cd fan-curve-app
   ```

2. **Build the application:**
   ```bash
   cargo build --release
   ```

3. **Run the GUI:**
   ```bash
   cargo run --release -- --gui
   ```

### One-Line Installation

```bash
curl -sSL https://raw.githubusercontent.com/yourusername/fan-curve-app/main/install.sh | bash
```

## Usage

### GUI Mode

Launch the graphical interface:
```bash
cargo run -- --gui
```

#### GUI Features:

1. **Select Fan Curve**: Choose from predefined curves (Standard, Threadripper 2, HEDT, Xeon)
2. **Edit Points**: Click "Edit" to modify temperature and fan duty values
3. **Remove Points**: Click "Remove" to delete unwanted points
4. **Add Points**: Use "Add Point" to create new curve points
5. **Test Mode**: Start real-time monitoring to see how your curve performs
6. **Save Profiles**: Save custom curves for future use

#### Predefined Fan Curves:

- **Standard**: Balanced performance and noise (0°C→0%, 30°C→20%, 70°C→60%, 100°C→100%)
- **Threadripper 2**: Optimized for high-performance CPUs
- **HEDT**: High-end desktop profile
- **Xeon**: Server-grade profile

### Command Line Mode

#### List Available Curves
```bash
cargo run -- fan-curve list
```

#### Get Current Curve
```bash
cargo run -- fan-curve get
```

#### Set Fan Curve
```bash
cargo run -- fan-curve set "Standard"
```

#### Add Custom Point
```bash
cargo run -- fan-curve add-point --temp 50 --duty 40
```

#### Test Fan Curve
```bash
cargo run -- fan-curve test --duration 60 --log-file fan_test.csv
```

#### Save Configuration
```bash
cargo run -- fan-curve save
```

#### Load Configuration
```bash
cargo run -- fan-curve load
```

### Daemon Mode

Run as a background service:
```bash
cargo run -- daemon
```

## Configuration

Configuration files are stored in `~/.fan_curve_app/config.json`. The app automatically creates a default configuration on first run.

### Configuration Structure

```json
{
  "curves": [
    {
      "name": "Standard",
      "points": [
        {"temp": 0, "duty": 0},
        {"temp": 30, "duty": 20},
        {"temp": 70, "duty": 60},
        {"temp": 100, "duty": 100}
      ]
    }
  ],
  "default_curve_index": 0
}
```

## Testing and Monitoring

### Real-time Testing

1. Select your desired fan curve
2. Click "Start Test" in the GUI
3. Monitor live temperature and fan data
4. Stop the test manually or let it complete

### Data Logging

Test data is automatically saved to CSV files:
- `fan_test.csv` - Default test log
- Custom log files can be specified via CLI

CSV format:
```csv
timestamp,temperature,fan_speed,fan_duty,cpu_usage
2024-01-15 10:30:00.123,45.2,1800,35,25.5
```

## Building from Source

### Development Build
```bash
cargo build
cargo run -- --gui
```

### Release Build
```bash
cargo build --release
cargo run --release -- --gui
```

### Cross-compilation

For different architectures:
```bash
# Install target
rustup target add x86_64-unknown-linux-gnu

# Build
cargo build --release --target x86_64-unknown-linux-gnu
```

## Troubleshooting

### Common Issues

1. **Permission Denied**: Run with appropriate permissions for hardware access
2. **GUI Not Starting**: Ensure X11 or Wayland is available
3. **Fan Data Not Updating**: Check if the daemon is running

### Debug Mode

Run with verbose logging:
```bash
cargo run -- --verbose --gui
```

### Log Files

Check application logs for detailed error information:
```bash
# Enable debug logging
RUST_LOG=debug cargo run -- --gui
```

## Development

### Project Structure

```
src/
├── main.rs              # Application entry point
├── lib.rs               # Library interface
├── args.rs              # CLI argument parsing
├── client.rs            # DBus client
├── daemon/              # Background service
├── fan.rs               # Fan curve data models
├── fan_curve_gui.rs     # GUI implementation
├── fan_monitor.rs       # Monitoring system
├── errors.rs            # Error handling
└── logging.rs           # Logging configuration
```

### Adding New Features

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

### Testing

Run the test suite:
```bash
cargo test
```

Run specific tests:
```bash
cargo test test_fan_curve_interpolation
```

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [egui](https://github.com/emilk/egui) for the GUI
- Uses [zbus](https://github.com/dbus2/zbus-rs) for DBus communication
- Inspired by System76 Power management tools

## Support

- 📧 Email: support@example.com
- 🐛 Issues: [GitHub Issues](https://github.com/yourusername/fan-curve-app/issues)
- 📖 Documentation: [Wiki](https://github.com/yourusername/fan-curve-app/wiki)

---

**Note**: This application is designed for System76 laptops and may require hardware-specific drivers for full functionality.
