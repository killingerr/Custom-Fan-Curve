#!/bin/bash

# Quick fresh install test
# Tests the most common scenarios

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

print_status() {
    echo "🔧 $1"
}

print_success() {
    echo "✅ $1"
}

print_error() {
    echo "❌ $1"
}

print_status "Running quick fresh install tests..."

# Test 1: Check if install script is executable
if [ ! -x "$SCRIPT_DIR/install.sh" ]; then
    print_error "Install script is not executable!"
    exit 1
fi
print_success "Install script is executable"

# Test 2: Check if Cargo.lock exists
if [ ! -f "$SCRIPT_DIR/Cargo.lock" ]; then
    print_error "Cargo.lock is missing! Run 'cargo build' first."
    exit 1
fi
print_success "Cargo.lock exists"

# Test 3: Check if required system dependencies are available
print_status "Checking system dependencies..."
MISSING_DEPS=()

for dep in "gcc" "pkg-config" "libdbus-1-dev" "libx11-dev"; do
    if ! dpkg -l | grep -q "^ii.*$dep"; then
        MISSING_DEPS+=("$dep")
    fi
done

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    print_error "Missing dependencies: ${MISSING_DEPS[*]}"
    print_status "Run: sudo apt-get install ${MISSING_DEPS[*]}"
    exit 1
fi
print_success "All system dependencies are available"

# Test 4: Check Rust installation
if ! command -v cargo &> /dev/null; then
    print_error "Rust/Cargo is not installed!"
    print_status "Install Rust from: https://rustup.rs/"
    exit 1
fi
print_success "Rust/Cargo is installed"

# Test 5: Test cargo build (dry run)
print_status "Testing cargo build..."
cd "$SCRIPT_DIR"
if cargo check --quiet; then
    print_success "Cargo build test passed"
else
    print_error "Cargo build test failed!"
    exit 1
fi

print_success "All quick tests passed! Ready for fresh install testing."
