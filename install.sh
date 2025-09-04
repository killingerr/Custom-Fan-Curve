#!/bin/bash

# Fan Curve App Installation Script
# This script installs the Fan Curve Control App on Linux systems

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
APP_NAME="fan-curve-app"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="$HOME/.fan_curve_app"
REPO_URL="https://github.com/yourusername/fan-curve-app.git"
TEMP_DIR="/tmp/fan-curve-app-install"

# Functions
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_requirements() {
    print_status "Checking system requirements..."
    
    # Check if running on Linux
    if [[ "$OSTYPE" != "linux-gnu"* ]]; then
        print_error "This script only supports Linux systems"
        exit 1
    fi
    
    # Check if Rust is installed
    if ! command -v cargo &> /dev/null; then
        print_error "Rust is not installed. Please install Rust first:"
        echo "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
    
    # Check Rust version
    RUST_VERSION=$(cargo --version | cut -d' ' -f2 | cut -d'.' -f1-2)
    REQUIRED_VERSION="1.75"
    if [ "$(printf '%s\n' "$REQUIRED_VERSION" "$RUST_VERSION" | sort -V | head -n1)" != "$REQUIRED_VERSION" ]; then
        print_error "Rust version $RUST_VERSION is too old. Please upgrade to 1.75 or later"
        exit 1
    fi
    
    print_success "System requirements check passed"
}

install_dependencies() {
    print_status "Installing system dependencies..."
    
    # Detect package manager and install dependencies
    if command -v apt-get &> /dev/null; then
        # Debian/Ubuntu
        sudo apt-get update
        sudo apt-get install -y build-essential pkg-config libssl-dev
    elif command -v yum &> /dev/null; then
        # RHEL/CentOS
        sudo yum groupinstall -y "Development Tools"
        sudo yum install -y openssl-devel pkgconfig
    elif command -v pacman &> /dev/null; then
        # Arch Linux
        sudo pacman -S --noconfirm base-devel openssl pkgconf
    elif command -v zypper &> /dev/null; then
        # openSUSE
        sudo zypper install -y gcc gcc-c++ make openssl-devel pkg-config
    else
        print_warning "Could not detect package manager. Please install build-essential and openssl-dev manually"
    fi
    
    print_success "Dependencies installed"
}

download_and_build() {
    print_status "Downloading and building $APP_NAME..."
    
    # Clean up any existing temp directory
    rm -rf "$TEMP_DIR"
    
    # Clone repository
    git clone "$REPO_URL" "$TEMP_DIR"
    cd "$TEMP_DIR"
    
    # Build in release mode
    print_status "Building application (this may take a few minutes)..."
    cargo build --release
    
    print_success "Application built successfully"
}

install_application() {
    print_status "Installing application to $INSTALL_DIR..."
    
    # Create install directory if it doesn't exist
    sudo mkdir -p "$INSTALL_DIR"
    
    # Copy binary
    sudo cp "target/release/$APP_NAME" "$INSTALL_DIR/"
    sudo chmod +x "$INSTALL_DIR/$APP_NAME"
    
    # Create symlink for easier access
    if [ ! -L "/usr/local/bin/fan-curve" ]; then
        sudo ln -s "$INSTALL_DIR/$APP_NAME" "/usr/local/bin/fan-curve"
    fi
    
    print_success "Application installed to $INSTALL_DIR"
}

setup_configuration() {
    print_status "Setting up configuration directory..."
    
    # Create config directory
    mkdir -p "$CONFIG_DIR"
    
    # Create default config if it doesn't exist
    if [ ! -f "$CONFIG_DIR/config.json" ]; then
        cat > "$CONFIG_DIR/config.json" << 'EOF'
{
  "curves": [
    {
      "name": "Standard",
      "points": [
        {"temp": 0, "duty": 0},
        {"temp": 30, "duty": 20},
        {"temp": 40, "duty": 30},
        {"temp": 50, "duty": 40},
        {"temp": 60, "duty": 50},
        {"temp": 70, "duty": 60},
        {"temp": 80, "duty": 70},
        {"temp": 90, "duty": 80},
        {"temp": 100, "duty": 100}
      ]
    },
    {
      "name": "Threadripper 2",
      "points": [
        {"temp": 0, "duty": 0},
        {"temp": 25, "duty": 10},
        {"temp": 35, "duty": 20},
        {"temp": 45, "duty": 30},
        {"temp": 55, "duty": 40},
        {"temp": 65, "duty": 50},
        {"temp": 75, "duty": 60},
        {"temp": 85, "duty": 70},
        {"temp": 95, "duty": 80},
        {"temp": 100, "duty": 100}
      ]
    }
  ],
  "default_curve_index": 0
}
EOF
        print_success "Default configuration created"
    else
        print_status "Configuration already exists, skipping"
    fi
}

create_desktop_entry() {
    print_status "Creating desktop entry..."
    
    # Create desktop entry for GUI
    cat > "$HOME/.local/share/applications/fan-curve-app.desktop" << EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Fan Curve Control
Comment=Control CPU fan curves on System76 laptops
Exec=/usr/local/bin/fan-curve --gui
Icon=applications-system
Terminal=false
Categories=System;Settings;
Keywords=fan;temperature;system76;hardware;
EOF
    
    # Make it executable
    chmod +x "$HOME/.local/share/applications/fan-curve-app.desktop"
    
    print_success "Desktop entry created"
}

cleanup() {
    print_status "Cleaning up temporary files..."
    rm -rf "$TEMP_DIR"
    print_success "Cleanup completed"
}

show_usage() {
    echo "Fan Curve Control App - Installation Script"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -h, --help     Show this help message"
    echo "  --no-gui       Skip desktop entry creation"
    echo "  --no-deps      Skip dependency installation"
    echo ""
    echo "Examples:"
    echo "  $0                    # Full installation"
    echo "  $0 --no-gui          # Install without desktop entry"
    echo "  $0 --no-deps         # Skip dependency installation"
}

main() {
    local skip_gui=false
    local skip_deps=false
    
    # Parse command line arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_usage
                exit 0
                ;;
            --no-gui)
                skip_gui=true
                shift
                ;;
            --no-deps)
                skip_deps=true
                shift
                ;;
            *)
                print_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
    done
    
    echo "=========================================="
    echo "  Fan Curve Control App Installer"
    echo "=========================================="
    echo ""
    
    # Run installation steps
    check_requirements
    
    if [ "$skip_deps" = false ]; then
        install_dependencies
    fi
    
    download_and_build
    install_application
    setup_configuration
    
    if [ "$skip_gui" = false ]; then
        create_desktop_entry
    fi
    
    cleanup
    
    echo ""
    echo "=========================================="
    print_success "Installation completed successfully!"
    echo "=========================================="
    echo ""
    echo "Usage:"
    echo "  fan-curve --gui              # Launch GUI"
    echo "  fan-curve --help             # Show help"
    echo "  fan-curve fan-curve list     # List available curves"
    echo ""
    echo "Configuration directory: $CONFIG_DIR"
    echo "Binary location: $INSTALL_DIR/$APP_NAME"
    echo ""
    echo "To uninstall, run:"
    echo "  sudo rm $INSTALL_DIR/$APP_NAME /usr/local/bin/fan-curve"
    echo "  rm -rf $CONFIG_DIR"
    echo "  rm $HOME/.local/share/applications/fan-curve-app.desktop"
}

# Run main function with all arguments
main "$@"
