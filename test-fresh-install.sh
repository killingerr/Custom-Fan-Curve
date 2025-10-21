#!/bin/bash

# Test fresh install script for Fan Curve App
# This script creates a clean environment and tests the installation

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTAINER_NAME="fan-curve-test-$(date +%s)"

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
    print_status "Cleaning up test container..."
    docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
}

# Set up cleanup on exit
trap cleanup EXIT

print_status "Testing fresh install in Docker container..."

# Create Dockerfile for testing
cat > "$SCRIPT_DIR/Dockerfile.test" << 'EOF'
FROM ubuntu:22.04

# Prevent interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install basic dependencies
RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    pkg-config \
    libdbus-1-dev \
    libx11-dev \
    libegl1 \
    libgl1 \
    mesa-utils \
    libusb-1.0-0-dev \
    devscripts \
    debhelper \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:$PATH"

# Create test user
RUN useradd -m -s /bin/bash testuser
USER testuser
WORKDIR /home/testuser

# Copy the repo
COPY --chown=testuser:testuser . /home/testuser/Fan-Curve-App/
WORKDIR /home/testuser/Fan-Curve-App

# Test the installation
RUN chmod +x install.sh
RUN ./install.sh

# Verify installation
RUN /usr/local/bin/fan-curve --help
EOF

print_status "Building test container..."
docker build -f "$SCRIPT_DIR/Dockerfile.test" -t fan-curve-test "$SCRIPT_DIR"

print_status "Running installation test..."
if docker run --name "$CONTAINER_NAME" fan-curve-test; then
    print_success "Fresh install test PASSED!"
    
    # Test the installed binary
    print_status "Testing installed binary..."
    if docker run --rm fan-curve-test /usr/local/bin/fan-curve --help > /dev/null 2>&1; then
        print_success "Binary test PASSED!"
    else
        print_error "Binary test FAILED!"
        exit 1
    fi
else
    print_error "Fresh install test FAILED!"
    exit 1
fi

print_success "All tests passed! Fresh install works correctly."
