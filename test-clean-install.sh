#!/bin/bash

# Local clean environment testing
# Creates a temporary user and tests installation

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_USER="fan-curve-test-$(date +%s)"
TEST_HOME="/tmp/$TEST_USER"

print_status() {
    echo "🔧 $1"
}

print_success() {
    echo "✅ $1"
}

print_error() {
    echo "❌ $1"
}

cleanup() {
    print_status "Cleaning up test environment..."
    sudo userdel -r "$TEST_USER" 2>/dev/null || true
    sudo rm -rf "$TEST_HOME" 2>/dev/null || true
}

# Set up cleanup on exit
trap cleanup EXIT

print_status "Creating clean test environment..."

# Create test user
sudo useradd -m -s /bin/bash -d "$TEST_HOME" "$TEST_USER"

# Install Rust for test user
print_status "Installing Rust for test user..."
sudo -u "$TEST_USER" bash -c "
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    export PATH=\"\$HOME/.cargo/bin:\$PATH\"
    rustup default stable
"

# Copy repo to test user's home
sudo cp -r "$SCRIPT_DIR" "$TEST_HOME/Fan-Curve-App"
sudo chown -R "$TEST_USER:$TEST_USER" "$TEST_HOME/Fan-Curve-App"

print_status "Testing installation as test user..."

# Run installation as test user
sudo -u "$TEST_USER" bash -c "
    cd '$TEST_HOME/Fan-Curve-App'
    export PATH=\"\$HOME/.cargo/bin:\$PATH\"
    chmod +x install.sh
    ./install.sh
"

# Test the installation
if sudo -u "$TEST_USER" /usr/local/bin/fan-curve --help > /dev/null 2>&1; then
    print_success "Clean environment test PASSED!"
else
    print_error "Clean environment test FAILED!"
    exit 1
fi

print_success "All clean environment tests passed!"
