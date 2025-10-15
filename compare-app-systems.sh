#!/bin/bash

# Compare Fan Curve App Systems Diagnostic Script
# Compares app binaries, DBus connectivity, and system differences

set -e

TIMESTAMP=$(date +"%Y%m%d-%H%M%S")
OUTPUT_DIR="app-comparison-${TIMESTAMP}"
mkdir -p "$OUTPUT_DIR"

echo "=== Fan Curve App System Comparison ==="
echo "Output directory: $OUTPUT_DIR"
echo "Timestamp: $(date)"
echo

# Function to run command and save output
run_and_save() {
    local description="$1"
    local command="$2"
    local output_file="$3"
    
    echo "Running: $description"
    echo "Command: $command" > "$output_file"
    echo "Timestamp: $(date)" >> "$output_file"
    echo "---" >> "$output_file"
    
    if eval "$command" >> "$output_file" 2>&1; then
        echo "✅ Success"
    else
        echo "❌ Failed (exit code: $?)"
    fi
    echo
}

# System Information
echo "=== System Information ==="
run_and_save "System info" "uname -a" "$OUTPUT_DIR/system-info.txt"
run_and_save "OS version" "lsb_release -a" "$OUTPUT_DIR/os-version.txt"
run_and_save "Kernel modules" "lsmod | grep -E '(system76|thelio|fan)'" "$OUTPUT_DIR/kernel-modules.txt"

# Hardware Detection
echo "=== Hardware Detection ==="
run_and_save "Hwmon devices" "ls -la /sys/class/hwmon/" "$OUTPUT_DIR/hwmon-devices.txt"
run_and_save "Thelio I/O detection" "find /sys/class/hwmon -name name -exec cat {} \; -exec dirname {} \;" "$OUTPUT_DIR/thelio-io-detection.txt"
run_and_save "PWM files" "find /sys/class/hwmon -name 'pwm*' -type f" "$OUTPUT_DIR/pwm-files.txt"

# Fan Curve App Binary Analysis
echo "=== Fan Curve App Binary Analysis ==="
run_and_save "System-installed app location" "which fan-curve-app" "$OUTPUT_DIR/app-location.txt"
run_and_save "System-installed app info" "file \$(which fan-curve-app)" "$OUTPUT_DIR/app-file-info.txt"
run_and_save "System-installed app size" "ls -la \$(which fan-curve-app)" "$OUTPUT_DIR/app-size.txt"
run_and_save "System-installed app strings" "strings \$(which fan-curve-app) | grep -E '(DBus|system76|fan)' | head -20" "$OUTPUT_DIR/app-strings.txt"

# Local build analysis (if exists)
if [ -f "./target/release/fan-curve-app" ]; then
    echo "=== Local Build Analysis ==="
    run_and_save "Local app info" "file ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-file-info.txt"
    run_and_save "Local app size" "ls -la ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-size.txt"
    run_and_save "Local app strings" "strings ./target/release/fan-curve-app | grep -E '(DBus|system76|fan)' | head -20" "$OUTPUT_DIR/local-app-strings.txt"
    
    # Compare binaries
    echo "=== Binary Comparison ==="
    if [ -f "$(which fan-curve-app)" ]; then
        run_and_save "Binary diff" "diff <(hexdump -C ./target/release/fan-curve-app) <(hexdump -C \$(which fan-curve-app))" "$OUTPUT_DIR/binary-diff.txt"
        run_and_save "Binary checksums" "md5sum ./target/release/fan-curve-app \$(which fan-curve-app)" "$OUTPUT_DIR/binary-checksums.txt"
    fi
fi

# DBus Analysis
echo "=== DBus Analysis ==="
run_and_save "DBus service status" "systemctl status com.system76.PowerDaemon.service" "$OUTPUT_DIR/dbus-service-status.txt"
run_and_save "DBus service logs" "journalctl -u com.system76.PowerDaemon.service --since '5 minutes ago' --no-pager" "$OUTPUT_DIR/dbus-service-logs.txt"
run_and_save "DBus interfaces" "dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply /com/system76/PowerDaemon/Fan org.freedesktop.DBus.Introspectable.Introspect" "$OUTPUT_DIR/dbus-interfaces.txt"

# Test DBus connectivity
echo "=== DBus Connectivity Tests ==="
run_and_save "DBus SetDuty test (255)" "timeout 5 dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply /com/system76/PowerDaemon/Fan com.system76.PowerDaemon.Fan.SetDuty byte:255" "$OUTPUT_DIR/dbus-setduty-255.txt"
run_and_save "DBus SetDuty test (128)" "timeout 5 dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply /com/system76/PowerDaemon/Fan com.system76.PowerDaemon.Fan.SetDuty byte:128" "$OUTPUT_DIR/dbus-setduty-128.txt"

# Library Dependencies
echo "=== Library Dependencies ==="
if [ -f "$(which fan-curve-app)" ]; then
    run_and_save "System app dependencies" "ldd \$(which fan-curve-app)" "$OUTPUT_DIR/system-app-dependencies.txt"
fi
if [ -f "./target/release/fan-curve-app" ]; then
    run_and_save "Local app dependencies" "ldd ./target/release/fan-curve-app" "$OUTPUT_DIR/local-app-dependencies.txt"
fi

# Package Information
echo "=== Package Information ==="
run_and_save "Installed packages" "dpkg -l | grep -E '(system76|dbus|libdbus)'" "$OUTPUT_DIR/installed-packages.txt"
run_and_save "Cargo version" "cargo --version" "$OUTPUT_DIR/cargo-version.txt"
run_and_save "Rust version" "rustc --version" "$OUTPUT_DIR/rust-version.txt"

# Environment Variables
echo "=== Environment Variables ==="
run_and_save "DBus environment" "env | grep -i dbus" "$OUTPUT_DIR/dbus-environment.txt"
run_and_save "PATH" "echo \$PATH" "$OUTPUT_DIR/path.txt"

# Process Information
echo "=== Process Information ==="
run_and_save "Running fan-curve processes" "ps aux | grep fan-curve" "$OUTPUT_DIR/fan-curve-processes.txt"
run_and_save "System76-power processes" "ps aux | grep system76-power" "$OUTPUT_DIR/system76-power-processes.txt"

# File Permissions
echo "=== File Permissions ==="
run_and_save "App permissions" "ls -la \$(which fan-curve-app) 2>/dev/null || echo 'App not found'" "$OUTPUT_DIR/app-permissions.txt"
run_and_save "PWM permissions" "ls -la /sys/class/hwmon/hwmon*/pwm* 2>/dev/null || echo 'PWM files not found'" "$OUTPUT_DIR/pwm-permissions.txt"

# Create summary
echo "=== Creating Summary ==="
cat > "$OUTPUT_DIR/SUMMARY.txt" << EOF
Fan Curve App System Comparison Summary
=====================================
Timestamp: $(date)
Output Directory: $OUTPUT_DIR

Key Files Generated:
- system-info.txt: System hardware and OS information
- app-location.txt: Location of system-installed fan-curve-app
- app-file-info.txt: File type and architecture of system app
- app-size.txt: Size and timestamps of system app
- app-strings.txt: Key strings found in system app binary
- dbus-service-status.txt: Status of system76-power daemon
- dbus-service-logs.txt: Recent daemon logs
- dbus-interfaces.txt: Available DBus interfaces
- dbus-setduty-255.txt: Test DBus SetDuty call (255)
- dbus-setduty-128.txt: Test DBus SetDuty call (128)
- system-app-dependencies.txt: Library dependencies of system app
- installed-packages.txt: Relevant installed packages

Comparison Instructions:
1. Run this script on both systems
2. Compare the generated files between systems
3. Look for differences in:
   - App binary size, timestamps, and strings
   - DBus connectivity test results
   - Library dependencies
   - System76-power daemon status and logs

Key Things to Check:
- Are the app binaries identical? (check binary-checksums.txt)
- Does DBus SetDuty work on both systems?
- Are the library dependencies the same?
- Is the system76-power daemon running and responsive on both?

EOF

echo "✅ Comparison complete!"
echo "📁 Results saved in: $OUTPUT_DIR"
echo "📋 Summary: $OUTPUT_DIR/SUMMARY.txt"
echo
echo "Next steps:"
echo "1. Copy this script to the Mega system"
echo "2. Run it on both systems"
echo "3. Compare the generated files to identify differences"
