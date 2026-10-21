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
REPO_URL="https://github.com/killingerr/Custom-Fan-Curve.git"
TEMP_DIR="/tmp/fan-curve-app-install"
# Checkout this script lives in, built by --local
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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
    
    # With rustup installed but no default toolchain, cargo exists but can't
    # run anything
    if command -v rustup &> /dev/null && ! cargo --version &> /dev/null; then
        print_warning "rustup has no default Rust toolchain. Setting it to stable..."
        rustup default stable
    fi

    # Check if Rust is installed
    if ! command -v cargo &> /dev/null; then
        print_warning "Rust is not installed. Installing Rust..."
        install_rust
    else
        # Check Rust version
        RUST_VERSION=$(cargo --version 2>/dev/null | cut -d' ' -f2 | cut -d'.' -f1-2)
        # egui 0.32 needs Rust 1.85
        REQUIRED_VERSION="1.85"
        if [ -z "$RUST_VERSION" ] || [ "$(printf '%s\n' "$REQUIRED_VERSION" "$RUST_VERSION" | sort -V | head -n1)" != "$REQUIRED_VERSION" ]; then
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
    if command -v rustup &> /dev/null; then
        rustup update stable
        rustup default stable
    else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    fi
    # Put rustup's cargo ahead of any older one installed by the system
    source "$HOME/.cargo/env"
    print_success "Rust $(cargo --version | cut -d' ' -f2) is ready"
}

install_dependencies() {
    print_step "Installing system dependencies..."
    
    # Detect package manager and install dependencies
    if command -v apt-get &> /dev/null; then
        # Debian/Ubuntu
        print_status "Detected apt package manager (Debian/Ubuntu)"
        sudo apt-get update
        sudo apt-get install -y build-essential pkg-config libssl-dev libx11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev git curl
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

build_local() {
    print_step "Building $APP_NAME from $SCRIPT_DIR..."

    # Later steps read data/ and assets/ relative to the source tree
    cd "$SCRIPT_DIR"

    print_status "Building application (this may take a few minutes)..."
    cargo build --release

    print_success "Application built successfully"
}

install_application() {
    print_step "Installing application to $INSTALL_DIR..."
    
    # Create install directory if it doesn't exist
    sudo mkdir -p "$INSTALL_DIR"
    
    # Copy binary. install(1) replaces the file instead of writing into it, so
    # this works while the daemon or notification agent is running it.
    sudo install -m 755 "target/release/$APP_NAME" "$INSTALL_DIR/$APP_NAME"

    # Create symlink for easier access
    sudo ln -sfn "$INSTALL_DIR/$APP_NAME" "/usr/local/bin/fan-curve"
    
    print_success "Application installed to $INSTALL_DIR"
}

setup_configuration() {
    print_step "Setting up configuration directory..."

    # No default config is written here: when none exists, the daemon and GUI
    # create one with system76-power's built-in curves (Standard, Threadripper 2,
    # HEDT, Xeon) and pick the default for this model the way system76-power does.
    mkdir -p "$CONFIG_DIR"

    print_success "Configuration directory ready"
}

install_daemon() {
    print_step "Installing fan curve daemon (DBus policy + systemd service)..."

    # DBus policy: allows root to own the service name and users to call it
    sudo cp "data/com.system76.FanCurveDaemon.conf" /usr/share/dbus-1/system.d/

    # Systemd unit
    sudo cp "data/fan-curve-daemon.service" /etc/systemd/system/

    # System-wide config for the daemon, seeded from the user config if there
    # is one; otherwise the daemon creates its own default on first start.
    sudo mkdir -p /etc/fan-curve-app
    if [ ! -f /etc/fan-curve-app/config.json ] && [ -f "$CONFIG_DIR/config.json" ]; then
        sudo cp "$CONFIG_DIR/config.json" /etc/fan-curve-app/config.json
    fi

    sudo systemctl daemon-reload
    # Pick up a new DBus policy (dbus-broker doesn't watch the directory)
    sudo systemctl reload dbus 2>/dev/null || true
    sudo systemctl enable fan-curve-daemon.service
    # Restart so an update runs the new binary; enable --now leaves an
    # already running daemon on the old one.
    sudo systemctl restart fan-curve-daemon.service

    if systemctl is-active --quiet fan-curve-daemon.service; then
        print_success "Fan curve daemon is running"
    else
        print_warning "Daemon may not be running. Check with: sudo systemctl status fan-curve-daemon"
    fi
}

# Pop!_OS staging repository with the system76-power build that can hand fan
# control to this app (ClaimFanControl, pop-os/system76-power PR #495), until a
# release includes it
S76_POWER_REPO="popdev:testing-system76-power-pr495"
S76_POWER_REPO_ID="popdev-testing-system76-power-pr495"

# Whether the installed system76-power can hand fan control to this app
system76_power_can_claim() {
    if busctl --system introspect com.system76.PowerDaemon /com/system76/PowerDaemon \
            com.system76.PowerDaemon 2> /dev/null | grep -q ClaimFanControl; then
        return 0
    fi
    # The service may not be running; check the program itself
    grep -qa ClaimFanControl /usr/bin/system76-power 2> /dev/null
}

ensure_system76_power() {
    # The daemon only drives Thelio Io boards, and only after system76-power
    # grants it the fan control claim, so claim support only matters there.
    if ! grep -qsx -e system76_thelio_io -e system76_io /sys/class/hwmon/*/name; then
        return 0
    fi
    print_step "Checking system76-power..."
    if system76_power_can_claim; then
        print_success "system76-power supports handing over fan control"
        return 0
    fi

    print_warning "This system76-power can't hand over fan control (no ClaimFanControl)."
    local os_id="" os_codename=""
    if [ -f /etc/os-release ]; then
        os_id=$(. /etc/os-release && echo "$ID")
        os_codename=$(. /etc/os-release && echo "$VERSION_CODENAME")
    fi
    if [ "$os_id" = pop ] && [ "$os_codename" = noble ] && command -v apt-manage &> /dev/null; then
        print_status "Installing system76-power from $S76_POWER_REPO..."
        if sudo apt-manage add -y "$S76_POWER_REPO" \
                && sudo apt-get update \
                && sudo apt-get install -y system76-power; then
            sudo systemctl restart com.system76.PowerDaemon || true
            sleep 2
            if system76_power_can_claim; then
                print_success "Installed system76-power $(dpkg-query -W -f='${Version}' system76-power)"
                return 0
            fi
        else
            # Don't leave a repository behind that apt can't use
            sudo apt-manage remove "$S76_POWER_REPO_ID" &> /dev/null || true
            sudo apt-get update &> /dev/null || true
        fi
        print_warning "Couldn't install a system76-power that supports fan control claim."
    else
        print_warning "Automatic system76-power install needs Pop!_OS 24.04."
    fi
    print_warning "Until it's updated, this app won't control the fans."
    print_warning "To build it from source:"
    print_warning "  git clone -b fan-control-claim-pr https://github.com/tonysystem76/system76-power.git"
    print_warning "  cd system76-power && make && sudo make install"
    print_warning "  sudo systemctl restart com.system76.PowerDaemon"
}

install_notifier() {
    print_step "Installing desktop notification agent..."

    # The daemon runs as root and can't reach the desktop, so a small agent in
    # the user's session turns its overheat/fan control signals into notifications.
    local autostart_dir="$HOME/.config/autostart"
    mkdir -p "$autostart_dir"
    cat > "$autostart_dir/fan-curve-app-notify.desktop" << EOF
[Desktop Entry]
Type=Application
Name=$APP_DISPLAY_NAME Notifications
Comment=Notifies when fans are forced to full speed or the fan curve isn't running
Exec=$INSTALL_DIR/$APP_NAME notify
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF

    # Restart it now so it runs the new binary without a fresh login
    pkill -u "$USER" -f "$INSTALL_DIR/$APP_NAME notify" 2>/dev/null || true
    if [ -n "$DBUS_SESSION_BUS_ADDRESS" ]; then
        nohup "$INSTALL_DIR/$APP_NAME" notify > /dev/null 2>&1 &
        print_success "Notification agent running (starts automatically at login)"
    else
        print_success "Notification agent will start at next login"
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
Comment=Control fan curves on System76 computers
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

# Stop and remove the daemon
sudo systemctl disable --now fan-curve-daemon.service 2>/dev/null || true
sudo rm -f /etc/systemd/system/fan-curve-daemon.service
sudo rm -f /usr/share/dbus-1/system.d/com.system76.FanCurveDaemon.conf
sudo rm -rf /etc/fan-curve-app
sudo systemctl daemon-reload

# Remove binary and symlink
sudo rm -f /usr/local/bin/fan-curve-app
sudo rm -f /usr/local/bin/fan-curve

# Remove configuration
rm -rf ~/.fan_curve_app

# Remove desktop entry
rm -f ~/.local/share/applications/fan-curve-app.desktop

# Stop and remove the notification agent
pkill -u "$USER" -f "bin/fan-curve-app notify" 2>/dev/null || true
rm -f ~/.config/autostart/fan-curve-app-notify.desktop

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
    echo "  --local          Build and install this checkout, including"
    echo "                   uncommitted changes, instead of cloning GitHub"
    echo "  --no-system76-power"
    echo "                   Don't install a system76-power that can hand over"
    echo "                   fan control on Thelio Io systems"
    echo ""
    echo "Examples:"
    echo "  $0                    # Full system installation"
    echo "  $0 --no-gui          # Install without desktop entry"
    echo "  $0 --no-deps         # Skip dependency installation"
    echo "  $0 --local           # Install the code in this folder"
}

show_completion_message() {
    echo ""
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}  Installation completed successfully!${NC}"
    echo -e "${GREEN}========================================${NC}"
    echo ""
    echo -e "${CYAN}Usage:${NC}"
    echo "  fan-curve --gui                       # Launch GUI"
    echo "  fan-curve --help                      # Show help"
    echo "  fan-curve fan-curve list              # List available curves"
    echo "  fan-curve fan-curve set \"Standard\"    # Apply a curve via the daemon"
    echo ""
    echo -e "${CYAN}Daemon:${NC}"
    echo "  Status:  sudo systemctl status fan-curve-daemon"
    echo "  Logs:    sudo journalctl -u fan-curve-daemon -f"
    echo "  Config:  /etc/fan-curve-app/config.json"
    echo ""
    echo -e "${CYAN}Notifications:${NC}"
    echo "  A background agent ($APP_NAME notify) starts at login and shows a desktop"
    echo "  notification if the fans are forced to full speed or your curve isn't running."
    echo ""
    echo -e "${CYAN}Curves:${NC}"
    echo "  On boot the daemon always loads the saved default curve."
    echo "  'Apply' switches for this session only; reboot returns to default."
    echo "  'Set as Default' (or: fan-curve fan-curve set-default NAME) persists across reboots."
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
    local local_build=false
    local skip_s76_power=false
    
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
                local_build=true
                shift
                ;;
            --no-system76-power)
                skip_s76_power=true
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
    fi
    
    if [ "$local_build" = true ]; then
        build_local
    else
        download_and_build
    fi
    install_application
    setup_configuration
    if [ "$skip_s76_power" = false ]; then
        ensure_system76_power
    fi
    install_daemon
    
    if [ "$skip_icon" = false ]; then
        install_icon
    fi
    
    if [ "$skip_gui" = false ]; then
        create_desktop_entry
        install_notifier
    fi
    
    create_uninstall_script
    cleanup
    show_completion_message
}

# Run main function with all arguments
main "$@"