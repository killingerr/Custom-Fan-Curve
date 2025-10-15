#!/bin/bash

# Fan Curve App - Complete Installation Script
# This script handles everything needed to install and run the Fan Curve Control App

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Configuration
APP_NAME="fan-curve-app"
APP_DISPLAY_NAME="Fan Curve Control"
APP_VERSION="0.1.0"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="$HOME/.fan_curve_app"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons"
REPO_URL="https://github.com/tonysystem76/Fan-Curve-App.git"
TEMP_DIR="/tmp/fan-curve-app-install"
S76_POWER_TEMP_DIR="/tmp/system76-power-install"

# Functions
print_header() {
    echo -e "${PURPLE}========================================${NC}"
    echo -e "${PURPLE}  Fan Curve Control App Installer${NC}"
    echo -e "${PURPLE}  Version: $APP_VERSION${NC}"
    echo -e "${PURPLE}========================================${NC}"
    echo ""
}

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

print_step() {
    echo -e "${CYAN}[STEP]${NC} $1"
}

check_requirements() {
    print_step "Checking system requirements..."
    
    # Check if running on Linux
    if [[ "$OSTYPE" != "linux-gnu"* ]]; then
        print_error "This script only supports Linux systems"
        exit 1
    fi
    
    # Check if running as root
    if [[ $EUID -eq 0 ]]; then
        print_error "Please do not run this script as root. It will ask for sudo when needed."
        exit 1
    fi
    
    # Check if Rust is installed
    if ! command -v cargo &> /dev/null; then
        print_warning "Rust is not installed. Installing Rust..."
        install_rust
    else
        # Check Rust version
        RUST_VERSION=$(cargo --version | cut -d' ' -f2 | cut -d'.' -f1-2)
        REQUIRED_VERSION="1.75"
        if [ "$(printf '%s\n' "$REQUIRED_VERSION" "$RUST_VERSION" | sort -V | head -n1)" != "$REQUIRED_VERSION" ]; then
            print_warning "Rust version $RUST_VERSION is too old. Updating Rust..."
            install_rust
        else
            print_success "Rust $RUST_VERSION is installed and compatible"
        fi
    fi
    
    print_success "System requirements check passed"
}

install_rust() {
    print_status "Installing/updating Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    print_success "Rust installed successfully"
}

install_dependencies() {
    print_step "Installing system dependencies..."
    
    # Detect package manager and install dependencies
    if command -v apt-get &> /dev/null; then
        # Debian/Ubuntu
        print_status "Detected apt package manager (Debian/Ubuntu)"
        sudo apt-get update
        sudo apt-get install -y build-essential pkg-config cargo libssl-dev libx11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libdbus-1-dev libegl1 libgl1 mesa-utils libusb-1.0-0-dev devscripts debhelper git curl
    elif command -v yum &> /dev/null; then
        # RHEL/CentOS
        print_status "Detected yum package manager (RHEL/CentOS)"
        sudo yum groupinstall -y "Development Tools"
        sudo yum install -y openssl-devel pkgconfig libX11-devel libxcb-devel git curl
    elif command -v dnf &> /dev/null; then
        # Fedora
        print_status "Detected dnf package manager (Fedora)"
        sudo dnf groupinstall -y "Development Tools"
        sudo dnf install -y openssl-devel pkgconfig libX11-devel libxcb-devel git curl
    elif command -v pacman &> /dev/null; then
        # Arch Linux
        print_status "Detected pacman package manager (Arch Linux)"
        sudo pacman -S --noconfirm base-devel openssl pkgconf libx11 libxcb git curl
    elif command -v zypper &> /dev/null; then
        # openSUSE
        print_status "Detected zypper package manager (openSUSE)"
        sudo zypper install -y gcc gcc-c++ make openssl-devel pkg-config libX11-devel libxcb-devel git curl
    else
        print_warning "Could not detect package manager. Please install the following manually:"
        echo "  - build-essential (gcc, make, etc.)"
        echo "  - openssl-dev"
        echo "  - libx11-dev"
        echo "  - libxcb-dev"
        echo "  - git"
        echo "  - curl"
        read -p "Press Enter to continue after installing dependencies manually..."
    fi
    
    print_success "Dependencies installed"
}

install_system76_power() {
    print_step "Installing System76 Power..."
    git clone https://github.com/tonysystem76/system76-power.git "$S76_POWER_TEMP_DIR/system76-power"
    cd "$S76_POWER_TEMP_DIR/system76-power"
    dpkg-buildpackage -us -uc
    sudo dpkg -i system76-power_*.deb
    print_success "System76 Power installed successfully"
}

download_and_build() {
    print_step "Downloading and building $APP_NAME..."
    
    # Clean up any existing temp directory
    rm -rf "$TEMP_DIR"
    
    # Clone repository
    print_status "Cloning repository..."
    git clone "$REPO_URL" "$TEMP_DIR"
    cd "$TEMP_DIR"
    
    # Build in release mode
    print_status "Building application (this may take a few minutes)..."
    cargo build --release
    
    print_success "Application built successfully"
}

install_application() {
    print_step "Installing application to $INSTALL_DIR..."
    
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
    print_step "Setting up configuration directory..."
    
    # Create config direc tory
    mkdir -p "$CONFIG_DIR"
    
    # Create default config if it doesn't exist
    if [ ! -f "$CONFIG_DIR/config.json" ]; then
        print_status "Creating default configuration..."
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
      "name": "Quiet",
      "points": [
        {"temp": 0, "duty": 0},
        {"temp": 35, "duty": 10},
        {"temp": 45, "duty": 20},
        {"temp": 55, "duty": 30},
        {"temp": 65, "duty": 40},
        {"temp": 75, "duty": 50},
        {"temp": 85, "duty": 60},
        {"temp": 95, "duty": 70},
        {"temp": 100, "duty": 80}
      ]
    },
    {
      "name": "Performance",
      "points": [
        {"temp": 0, "duty": 0},
        {"temp": 25, "duty": 20},
        {"temp": 35, "duty": 30},
        {"temp": 45, "duty": 40},
        {"temp": 55, "duty": 50},
        {"temp": 65, "duty": 60},
        {"temp": 75, "duty": 70},
        {"temp": 85, "duty": 80},
        {"temp": 95, "duty": 90},
        {"temp": 100, "duty": 100}
      ]
    }
  ],
  "default_curve_index": 0,
  "auto_start": false,
  "polling_interval": 1000
}
EOF
        print_success "Default configuration created"
    else
        print_status "Configuration already exists, skipping"
    fi
}

install_icon() {
    print_step "Installing application icon..."
    
    # Create icon directory
    mkdir -p "$ICON_DIR"
    
    # Copy icon
    if [ -f "assets/fan-curve-app.svg" ]; then
        cp "assets/fan-curve-app.svg" "$ICON_DIR/"
        print_success "Application icon installed"
    else
        print_warning "Icon file not found, using system default"
    fi
}

create_desktop_entry() {
    print_step "Creating desktop entry..."
    
    # Create desktop directory
    mkdir -p "$DESKTOP_DIR"
    
    # Determine icon path
    if [ -f "$ICON_DIR/fan-curve-app.svg" ]; then
        ICON_PATH="$ICON_DIR/fan-curve-app.svg"
    else
        ICON_PATH="applications-system"
    fi
    
    # Create desktop entry for GUI
    cat > "$DESKTOP_DIR/fan-curve-app.desktop" << EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=$APP_DISPLAY_NAME
Comment=Control CPU fan curves on System76 laptops
Exec=/usr/local/bin/fan-curve --gui
Icon=$ICON_PATH
Terminal=false
Categories=System;Settings;HardwareSettings;
Keywords=fan;temperature;system76;hardware;cooling;
StartupNotify=true
MimeType=
EOF
    
    # Make it executable
    chmod +x "$DESKTOP_DIR/fan-curve-app.desktop"
    
    # Update desktop database
    if command -v update-desktop-database &> /dev/null; then
        update-desktop-database "$DESKTOP_DIR"
    fi
    
    print_success "Desktop entry created"
}

create_uninstall_script() {
    print_step "Creating uninstall script..."
    
    cat > "$HOME/uninstall-fan-curve-app.sh" << 'EOF'
#!/bin/bash

# Fan Curve App Uninstaller

echo "Uninstalling Fan Curve Control App..."

# Remove binary and symlink
sudo rm -f /usr/local/bin/fan-curve-app
sudo rm -f /usr/local/bin/fan-curve

# Remove configuration
rm -rf ~/.fan_curve_app

# Remove desktop entry
rm -f ~/.local/share/applications/fan-curve-app.desktop

# Remove icon
rm -f ~/.local/share/icons/fan-curve-app.svg

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database ~/.local/share/applications
fi

echo "Fan Curve Control App has been uninstalled."
EOF
    
    chmod +x "$HOME/uninstall-fan-curve-app.sh"
    print_success "Uninstall script created at $HOME/uninstall-fan-curve-app.sh"
}

cleanup() {
    print_step "Cleaning up temporary files..."
    rm -rf "$TEMP_DIR"
    print_success "Cleanup completed"
}

show_usage() {
    echo "Fan Curve Control App - Complete Installation Script"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -h, --help       Show this help message"
    echo "  --no-gui         Skip desktop entry creation"
    echo "  --no-deps        Skip dependency installation"
    echo "  --no-icon        Skip icon installation"
    echo "  --local          Install to local directory instead of system"
    echo ""
    echo "Examples:"
    echo "  $0                    # Full system installation"
    echo "  $0 --local            # Install to local directory"
    echo "  $0 --no-gui          # Install without desktop entry"
    echo "  $0 --no-deps         # Skip dependency installation"
}

show_completion_message() {
    echo ""
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}  Installation completed successfully!${NC}"
    echo -e "${GREEN}========================================${NC}"
    echo ""
    echo -e "${CYAN}Usage:${NC}"
    echo "  fan-curve --gui              # Launch GUI"
    echo "  fan-curve --help             # Show help"
    echo "  fan-curve list               # List available curves"
    echo ""
    echo -e "${CYAN}File locations:${NC}"
    echo "  Binary: $INSTALL_DIR/$APP_NAME"
    echo "  Config: $CONFIG_DIR"
    echo "  Desktop: $DESKTOP_DIR/fan-curve-app.desktop"
    if [ -f "$ICON_DIR/fan-curve-app.svg" ]; then
        echo "  Icon: $ICON_DIR/fan-curve-app.svg"
    fi
    echo ""
    echo -e "${CYAN}To uninstall:${NC}"
    echo "  $HOME/uninstall-fan-curve-app.sh"
    echo ""
    echo -e "${YELLOW}Note:${NC} You may need to log out and back in for the desktop entry to appear in your applications menu."
    echo ""
}

main() {
    local skip_gui=false
    local skip_deps=false
    local skip_icon=false
    local local_install=false
    
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
            --no-icon)
                skip_icon=true
                shift
                ;;
            --local)
                local_install=true
                INSTALL_DIR="$HOME/.local/bin"
                shift
                ;;
            *)
                print_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
        esac
    done
    
    print_header
    
    # Run installation steps
    check_requirements
    
    if [ "$skip_deps" = false ]; then
        install_dependencies
        install_system76_power
    fi
    
    download_and_build
    install_application
    setup_configuration
    
    if [ "$skip_icon" = false ]; then
        install_icon
    fi
    
    if [ "$skip_gui" = false ]; then
        create_desktop_entry
    fi
    
    create_uninstall_script
    cleanup
    show_completion_message
}

# Run main function with all arguments
main "$@"