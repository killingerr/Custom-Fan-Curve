#!/bin/bash

# Comprehensive Fan Curve App System Comparison Script
# Compares everything between working system and Mega to identify differences

set -euo pipefail

SCRIPT_NAME=$(basename "$0")
TIMESTAMP=$(date +"%Y%m%d-%H%M%S")
OUTPUT_DIR="fan-app-deep-comparison-${TIMESTAMP}"
mkdir -p "$OUTPUT_DIR"

print_header() {
    echo "=========================================="
    echo "  Fan Curve App Deep System Comparison"
    echo "  Timestamp: $(date)"
    echo "  Output: $OUTPUT_DIR"
    echo "=========================================="
    echo
}

print_step() {
    echo "[STEP] $1"
}

print_info() {
    echo "[INFO] $1"
}

print_success() {
    echo "[OK] $1"
}

# Function to run command and save output with metadata
run_and_save() {
    local description="$1"
    local command="$2"
    local output_file="$3"
    
    print_info "Running: $description"
    echo "Command: $command" > "$output_file"
    echo "Timestamp: $(date)" >> "$output_file"
    echo "Hostname: $(hostname)" >> "$output_file"
    echo "User: $(whoami)" >> "$output_file"
    echo "Working Directory: $(pwd)" >> "$output_file"
    echo "---" >> "$output_file"
    
    if eval "$command" >> "$output_file" 2>&1; then
        print_success "Completed: $description"
    else
        echo "❌ Failed: $description (exit code: $?)" >> "$output_file"
        print_info "Completed with errors: $description"
    fi
    echo
}

# System Information
print_step "System Information"
run_and_save "System info" "uname -a" "$OUTPUT_DIR/system-info.txt"
run_and_save "OS version" "lsb_release -a 2>/dev/null || cat /etc/os-release" "$OUTPUT_DIR/os-version.txt"
run_and_save "Kernel version" "uname -r" "$OUTPUT_DIR/kernel-version.txt"
run_and_save "Architecture" "uname -m" "$OUTPUT_DIR/architecture.txt"
run_and_save "CPU info" "lscpu | head -20" "$OUTPUT_DIR/cpu-info.txt"
run_and_save "Memory info" "free -h" "$OUTPUT_DIR/memory-info.txt"

# Rust Toolchain
print_step "Rust Toolchain Analysis"
run_and_save "Rust version" "rustc --version" "$OUTPUT_DIR/rust-version.txt"
run_and_save "Cargo version" "cargo --version" "$OUTPUT_DIR/cargo-version.txt"
run_and_save "Rustup show" "rustup show" "$OUTPUT_DIR/rustup-show.txt"
run_and_save "Rustup toolchain list" "rustup toolchain list" "$OUTPUT_DIR/rustup-toolchains.txt"
run_and_save "Rustup default" "rustup default" "$OUTPUT_DIR/rustup-default.txt"

# Fan Curve App Analysis
print_step "Fan Curve App Analysis"
run_and_save "App location" "which fan-curve-app" "$OUTPUT_DIR/app-location.txt"
run_and_save "App file info" "file \$(which fan-curve-app)" "$OUTPUT_DIR/app-file-info.txt"
run_and_save "App size and permissions" "ls -la \$(which fan-curve-app)" "$OUTPUT_DIR/app-size-perms.txt"
run_and_save "App checksum" "md5sum \$(which fan-curve-app)" "$OUTPUT_DIR/app-checksum.txt"

# App Binary Deep Analysis
print_step "App Binary Deep Analysis"
run_and_save "App strings (first 200)" "strings \$(which fan-curve-app) | head -200" "$OUTPUT_DIR/app-strings.txt"
run_and_save "App symbols" "nm -D \$(which fan-curve-app)" "$OUTPUT_DIR/app-symbols.txt"
run_and_save "App dynamic section" "readelf -d \$(which fan-curve-app)" "$OUTPUT_DIR/app-dynamic.txt"
run_and_save "App sections" "readelf -S \$(which fan-curve-app)" "$OUTPUT_DIR/app-sections.txt"
run_and_save "App dependencies" "ldd \$(which fan-curve-app)" "$OUTPUT_DIR/app-dependencies.txt"

# Local Build Analysis (if exists)
if [ -f "./target/release/fan-curve-app" ]; then
    print_step "Local Build Analysis"
    run_and_save "Local app file info" "file ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-file-info.txt"
    run_and_save "Local app size" "ls -la ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-size.txt"
    run_and_save "Local app checksum" "md5sum ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-checksum.txt"
    run_and_save "Local app dependencies" "ldd ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-dependencies.txt"
    
    # Compare binaries
    run_and_save "Binary comparison" "diff <(hexdump -C \$(which fan-curve-app)) <(hexdump -C ./target/release/fan-curve-app) || echo 'Binaries differ'" "$OUTPUT_DIR/binary-comparison.txt"
fi

# Repository Analysis
print_step "Repository Analysis"
if [ -d ".git" ]; then
    run_and_save "Git status" "git status" "$OUTPUT_DIR/git-status.txt"
    run_and_save "Git log (last 5)" "git log --oneline -5" "$OUTPUT_DIR/git-log.txt"
    run_and_save "Git branch" "git branch -a" "$OUTPUT_DIR/git-branches.txt"
    run_and_save "Git remote" "git remote -v" "$OUTPUT_DIR/git-remotes.txt"
    run_and_save "Git commit hash" "git rev-parse HEAD" "$OUTPUT_DIR/git-commit.txt"
fi

# Cargo Analysis
print_step "Cargo Analysis"
if [ -f "Cargo.toml" ]; then
    run_and_save "Cargo.toml" "cat Cargo.toml" "$OUTPUT_DIR/cargo-toml.txt"
fi
if [ -f "Cargo.lock" ]; then
    run_and_save "Cargo.lock (first 100 lines)" "head -100 Cargo.lock" "$OUTPUT_DIR/cargo-lock-head.txt"
    run_and_save "Cargo.lock dependencies" "grep -E '^name =|^version =' Cargo.lock | head -50" "$OUTPUT_DIR/cargo-lock-deps.txt"
fi
if [ -f "rust-toolchain.toml" ]; then
    run_and_save "Rust toolchain" "cat rust-toolchain.toml" "$OUTPUT_DIR/rust-toolchain.txt"
fi

# System76 Power Analysis
print_step "System76 Power Analysis"
run_and_save "System76-power service status" "systemctl status com.system76.PowerDaemon.service" "$OUTPUT_DIR/s76-power-status.txt"
run_and_save "System76-power version" "system76-power --version 2>/dev/null || echo 'Not available'" "$OUTPUT_DIR/s76-power-version.txt"
run_and_save "System76-power location" "which system76-power" "$OUTPUT_DIR/s76-power-location.txt"
run_and_save "System76-power daemon logs" "journalctl -u com.system76.PowerDaemon.service --since '10 minutes ago' --no-pager" "$OUTPUT_DIR/s76-power-logs.txt"

# DBus Analysis
print_step "DBus Analysis"
run_and_save "DBus service list" "dbus-send --system --dest=org.freedesktop.DBus --type=method_call --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames" "$OUTPUT_DIR/dbus-services.txt"
run_and_save "DBus System76 interfaces" "dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply /com/system76/PowerDaemon/Fan org.freedesktop.DBus.Introspectable.Introspect" "$OUTPUT_DIR/dbus-s76-interfaces.txt"
run_and_save "DBus SetDuty test (128)" "timeout 5 dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply /com/system76/PowerDaemon/Fan com.system76.PowerDaemon.Fan.SetDuty byte:128" "$OUTPUT_DIR/dbus-setduty-128.txt"
run_and_save "DBus SetDuty test (255)" "timeout 5 dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply /com/system76/PowerDaemon/Fan com.system76.PowerDaemon.Fan.SetDuty byte:255" "$OUTPUT_DIR/dbus-setduty-255.txt"

# Hardware Detection
print_step "Hardware Detection"
run_and_save "Hwmon devices" "ls -la /sys/class/hwmon/" "$OUTPUT_DIR/hwmon-devices.txt"
run_and_save "Hwmon names" "find /sys/class/hwmon -name name -exec cat {} \; -exec dirname {} \;" "$OUTPUT_DIR/hwmon-names.txt"
run_and_save "Thelio I/O detection" "find /sys/class/hwmon -name name -exec sh -c 'echo \"\$(cat \$1): \$1\"' _ {} \; | grep -i thelio" "$OUTPUT_DIR/thelio-detection.txt"
run_and_save "PWM files" "find /sys/class/hwmon -name 'pwm*' -type f" "$OUTPUT_DIR/pwm-files.txt"
run_and_save "Fan files" "find /sys/class/hwmon -name 'fan*' -type f" "$OUTPUT_DIR/fan-files.txt"

# Environment Analysis
print_step "Environment Analysis"
run_and_save "Environment variables" "env | sort" "$OUTPUT_DIR/environment.txt"
run_and_save "PATH" "echo \$PATH" "$OUTPUT_DIR/path.txt"
run_and_save "LD_LIBRARY_PATH" "echo \$LD_LIBRARY_PATH" "$OUTPUT_DIR/ld-library-path.txt"
run_and_save "RUST_LOG" "echo \$RUST_LOG" "$OUTPUT_DIR/rust-log.txt"

# Package Analysis
print_step "Package Analysis"
run_and_save "Installed packages (system76)" "dpkg -l | grep -i system76" "$OUTPUT_DIR/packages-system76.txt"
run_and_save "Installed packages (dbus)" "dpkg -l | grep -i dbus" "$OUTPUT_DIR/packages-dbus.txt"
run_and_save "Installed packages (rust)" "dpkg -l | grep -i rust" "$OUTPUT_DIR/packages-rust.txt"
run_and_save "Installed packages (cargo)" "dpkg -l | grep -i cargo" "$OUTPUT_DIR/packages-cargo.txt"

# Process Analysis
print_step "Process Analysis"
run_and_save "Fan curve processes" "ps aux | grep fan-curve" "$OUTPUT_DIR/processes-fan-curve.txt"
run_and_save "System76-power processes" "ps aux | grep system76-power" "$OUTPUT_DIR/processes-system76-power.txt"
run_and_save "DBus processes" "ps aux | grep dbus" "$OUTPUT_DIR/processes-dbus.txt"

# Runtime Test
print_step "Runtime Test"
run_and_save "App version check" "fan-curve-app --version 2>&1 || echo 'Version check failed'" "$OUTPUT_DIR/app-version.txt"
run_and_save "App help" "fan-curve-app --help 2>&1 || echo 'Help failed'" "$OUTPUT_DIR/app-help.txt"

# Create comparison summary
print_step "Creating Comparison Summary"
cat > "$OUTPUT_DIR/COMPARISON_SUMMARY.txt" << EOF
Fan Curve App Deep System Comparison Summary
==========================================
Timestamp: $(date)
Output Directory: $OUTPUT_DIR
Hostname: $(hostname)

Key Files Generated:
- system-info.txt: System hardware and OS information
- rust-version.txt: Rust toolchain version
- cargo-version.txt: Cargo version
- app-location.txt: Location of fan-curve-app binary
- app-file-info.txt: Binary file type and architecture
- app-checksum.txt: MD5 checksum of system binary
- app-dependencies.txt: Library dependencies of system binary
- app-strings.txt: Strings found in system binary
- app-symbols.txt: Symbols in system binary
- app-dynamic.txt: Dynamic section of system binary
- s76-power-status.txt: System76-power daemon status
- s76-power-logs.txt: Recent daemon logs
- dbus-services.txt: Available DBus services
- dbus-s76-interfaces.txt: System76-power DBus interfaces
- dbus-setduty-128.txt: DBus SetDuty test (128)
- dbus-setduty-255.txt: DBus SetDuty test (255)
- hwmon-devices.txt: Available hwmon devices
- hwmon-names.txt: Hwmon device names
- thelio-detection.txt: Thelio I/O detection results
- pwm-files.txt: Available PWM files
- fan-files.txt: Available fan files
- environment.txt: Environment variables
- packages-system76.txt: Installed System76 packages
- packages-dbus.txt: Installed DBus packages
- processes-fan-curve.txt: Running fan-curve processes
- processes-system76-power.txt: Running system76-power processes
- app-version.txt: App version output
- app-help.txt: App help output

Comparison Instructions:
1. Run this script on both systems
2. Compare the generated files between systems
3. Look for differences in:
   - App binary checksums and dependencies
   - Rust toolchain versions
   - System76-power daemon status and logs
   - DBus connectivity test results
   - Hardware detection results
   - Environment variables
   - Package versions

Key Things to Check:
- Are the app binaries identical? (compare app-checksum.txt)
- Does DBus SetDuty work on both systems? (check dbus-setduty-*.txt)
- Are the library dependencies the same? (compare app-dependencies.txt)
- Is the system76-power daemon running and responsive? (check s76-power-status.txt)
- Are hwmon devices detected the same way? (compare hwmon-*.txt)
- Are environment variables different? (compare environment.txt)
- Are package versions different? (compare packages-*.txt)

EOF

print_success "Deep comparison complete!"
echo "📁 Results saved in: $OUTPUT_DIR"
echo "📋 Summary: $OUTPUT_DIR/COMPARISON_SUMMARY.txt"
echo
echo "Next steps:"
echo "1. Copy this script to the Mega system"
echo "2. Run it on both systems"
echo "3. Compare the generated files to identify differences"
echo "4. Focus on app-checksum.txt, dbus-setduty-*.txt, and s76-power-logs.txt"
