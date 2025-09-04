#!/bin/bash

# Fan Curve App Launcher
# This script provides an easy way to launch the Fan Curve Control App

set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

APP_NAME="fan-curve-app"
BINARY_PATH="/usr/local/bin/fan-curve"

print_header() {
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}  Fan Curve Control App${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo ""
}

print_usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --gui, -g        Launch GUI (default)"
    echo "  --help, -h       Show this help"
    echo "  --list, -l       List available fan curves"
    echo "  --apply, -a      Apply a specific curve"
    echo "  --status, -s     Show current status"
    echo ""
    echo "Examples:"
    echo "  $0                    # Launch GUI"
    echo "  $0 --list            # List curves"
    echo "  $0 --apply Standard  # Apply Standard curve"
    echo "  $0 --status          # Show status"
}

check_installation() {
    if [ ! -f "$BINARY_PATH" ]; then
        echo -e "${RED}Error: Fan Curve App is not installed.${NC}"
        echo "Please run the installation script first:"
        echo "  curl -sSL https://raw.githubusercontent.com/tonysystem76/Fan-Curve-App/main/install.sh | bash"
        exit 1
    fi
}

launch_gui() {
    print_header
    echo -e "${GREEN}Launching Fan Curve Control GUI...${NC}"
    echo ""
    exec "$BINARY_PATH" --gui
}

show_status() {
    print_header
    echo -e "${YELLOW}Current Status:${NC}"
    echo ""
    "$BINARY_PATH" --status 2>/dev/null || echo "Status information not available"
}

list_curves() {
    print_header
    echo -e "${YELLOW}Available Fan Curves:${NC}"
    echo ""
    "$BINARY_PATH" list 2>/dev/null || echo "No curves available"
}

apply_curve() {
    local curve_name="$1"
    if [ -z "$curve_name" ]; then
        echo -e "${RED}Error: Please specify a curve name${NC}"
        echo "Usage: $0 --apply \"Curve Name\""
        exit 1
    fi
    
    print_header
    echo -e "${GREEN}Applying curve: $curve_name${NC}"
    echo ""
    "$BINARY_PATH" apply "$curve_name"
}

main() {
    # Check if app is installed
    check_installation
    
    # Parse arguments
    case "${1:-}" in
        --gui|-g|"")
            launch_gui
            ;;
        --help|-h)
            print_usage
            ;;
        --list|-l)
            list_curves
            ;;
        --apply|-a)
            apply_curve "$2"
            ;;
        --status|-s)
            show_status
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo ""
            print_usage
            exit 1
            ;;
    esac
}

main "$@"
