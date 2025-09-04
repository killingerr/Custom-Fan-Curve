# Fan Curve App Makefile
# Provides easy commands for building, testing, and installing the application

.PHONY: help build release test clean install uninstall run-gui run-cli

# Default target
help:
	@echo "Fan Curve Control App - Available Commands"
	@echo "=========================================="
	@echo ""
	@echo "Building:"
	@echo "  make build     - Build in debug mode"
	@echo "  make release   - Build in release mode"
	@echo "  make clean     - Clean build artifacts"
	@echo ""
	@echo "Running:"
	@echo "  make run-gui   - Run GUI application"
	@echo "  make run-cli   - Run CLI application"
	@echo ""
	@echo "Testing:"
	@echo "  make test      - Run all tests"
	@echo "  make test-unit - Run unit tests only"
	@echo ""
	@echo "Installation:"
	@echo "  make install   - Install to system"
	@echo "  make uninstall - Remove from system"
	@echo ""
	@echo "Development:"
	@echo "  make check     - Run cargo check"
	@echo "  make fmt       - Format code"
	@echo "  make clippy    - Run clippy linter"

# Build targets
build:
	@echo "Building in debug mode..."
	cargo build

release:
	@echo "Building in release mode..."
	cargo build --release

# Run targets
run-gui: build
	@echo "Running GUI application..."
	cargo run -- --gui

run-cli: build
	@echo "Running CLI application..."
	cargo run -- --help

# Test targets
test:
	@echo "Running all tests..."
	cargo test

test-unit:
	@echo "Running unit tests..."
	cargo test --lib

# Development targets
check:
	@echo "Running cargo check..."
	cargo check

fmt:
	@echo "Formatting code..."
	cargo fmt

clippy:
	@echo "Running clippy linter..."
	cargo clippy -- -D warnings

# Installation targets
install: release
	@echo "Installing application..."
	sudo cp target/release/fan-curve-app /usr/local/bin/
	sudo chmod +x /usr/local/bin/fan-curve-app
	@if [ ! -L /usr/local/bin/fan-curve ]; then \
		sudo ln -s /usr/local/bin/fan-curve-app /usr/local/bin/fan-curve; \
	fi
	@echo "Creating configuration directory..."
	@mkdir -p ~/.fan_curve_app
	@echo "Installation completed!"
	@echo "Usage: fan-curve --gui"

uninstall:
	@echo "Uninstalling application..."
	@sudo rm -f /usr/local/bin/fan-curve-app /usr/local/bin/fan-curve
	@echo "Application uninstalled!"

# Clean target
clean:
	@echo "Cleaning build artifacts..."
	cargo clean

# Development workflow
dev: fmt clippy test
	@echo "Development checks completed!"

# Quick start
quick: build run-gui

# All-in-one build and test
all: clean fmt clippy test release
	@echo "Full build and test completed!"
