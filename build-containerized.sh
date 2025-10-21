#!/bin/bash

# Containerized build to avoid all dependency conflicts
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTAINER_NAME="fan-curve-build-$(date +%s)"

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
    print_status "Cleaning up build container..."
    docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
}

trap cleanup EXIT

print_status "Building fan-curve-app in isolated container..."

# Create Dockerfile for isolated build
cat > "$SCRIPT_DIR/Dockerfile.build" << 'EOF'
FROM rust:1.75-slim

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libdbus-1-dev \
    libx11-dev \
    libegl1 \
    libgl1 \
    mesa-utils \
    libusb-1.0-0-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# Build with static linking
RUN cargo build --release --locked

# Create a minimal runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libdbus-1-3 \
    libx11-6 \
    libegl1 \
    libgl1 \
    libusb-1.0-0 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=0 /app/target/release/fan-curve-app /usr/local/bin/fan-curve-app
RUN ln -s /usr/local/bin/fan-curve-app /usr/local/bin/fan-curve

CMD ["fan-curve", "--help"]
EOF

print_status "Building container image..."
docker build -f "$SCRIPT_DIR/Dockerfile.build" -t fan-curve-app:latest "$SCRIPT_DIR"

print_status "Extracting binary from container..."
docker create --name "$CONTAINER_NAME" fan-curve-app:latest
docker cp "$CONTAINER_NAME:/usr/local/bin/fan-curve-app" "$SCRIPT_DIR/target/release/fan-curve-app"

print_success "Binary built successfully in isolated environment!"
print_status "Binary location: $SCRIPT_DIR/target/release/fan-curve-app"

# Test the binary
if "$SCRIPT_DIR/target/release/fan-curve-app" --help > /dev/null 2>&1; then
    print_success "Binary test passed!"
else
    print_error "Binary test failed!"
    exit 1
fi
