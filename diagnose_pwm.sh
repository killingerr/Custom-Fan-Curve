#!/bin/bash

echo "🔍 PWM Diagnostic Script for Fan Control Issue"
echo "=============================================="
echo ""

# Find the correct hwmon device
echo "1. Finding System76 hwmon device..."
HWMON_DEVICE=""
for hwmon in /sys/class/hwmon/hwmon*; do
    if [ -f "$hwmon/name" ]; then
        name=$(cat "$hwmon/name")
        if [[ "$name" == "system76"* ]] || [[ "$name" == "system76_thelio_io" ]]; then
            HWMON_DEVICE="$hwmon"
            echo "   ✅ Found: $hwmon -> $name"
            break
        fi
    fi
done

if [ -z "$HWMON_DEVICE" ]; then
    echo "   ❌ System76 hwmon device not found!"
    exit 1
fi

echo ""
echo "2. Checking PWM files..."
PWM_FILE="$HWMON_DEVICE/pwm1"
ENABLE_FILE="$HWMON_DEVICE/pwm1_enable"

if [ -f "$PWM_FILE" ]; then
    echo "   ✅ PWM file exists: $PWM_FILE"
    echo "   📊 Current PWM value: $(cat $PWM_FILE)"
else
    echo "   ❌ PWM file not found: $PWM_FILE"
    exit 1
fi

if [ -f "$ENABLE_FILE" ]; then
    echo "   ✅ Enable file exists: $ENABLE_FILE"
    echo "   📊 Current enable value: $(cat $ENABLE_FILE)"
else
    echo "   ⚠️  Enable file not found: $ENABLE_FILE"
fi

echo ""
echo "3. Testing PWM control..."

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "   ⚠️  Not running as root. PWM control may fail."
    echo "   💡 Run with: sudo $0"
fi

echo "   🔧 Setting PWM to 255 (100%)..."
echo "255" | sudo tee "$PWM_FILE" > /dev/null

echo "   🔧 Enabling manual PWM control..."
echo "1" | sudo tee "$ENABLE_FILE" > /dev/null

echo ""
echo "4. Verifying PWM values..."
sleep 1  # Give hardware time to process

echo "   📊 PWM value after setting: $(cat $PWM_FILE)"
echo "   📊 Enable value after setting: $(cat $ENABLE_FILE)"

echo ""
echo "5. Checking fan speed..."
FAN_INPUT="$HWMON_DEVICE/fan1_input"
if [ -f "$FAN_INPUT" ]; then
    echo "   📊 Fan speed: $(cat $FAN_INPUT) RPM"
else
    echo "   ❌ Fan input file not found: $FAN_INPUT"
fi

echo ""
echo "6. Monitoring PWM value over time..."
echo "   Watching for 10 seconds to see if PWM gets reset..."
for i in {1..10}; do
    pwm_val=$(cat "$PWM_FILE")
    enable_val=$(cat "$ENABLE_FILE")
    echo "   [$i] PWM: $pwm_val, Enable: $enable_val"
    sleep 1
done

echo ""
echo "7. Checking for competing processes..."
echo "   Processes that might be controlling fans:"
ps aux | grep -E "(system76|fan|pwm)" | grep -v grep || echo "   (none found)"

echo ""
echo "8. Checking system76-power status..."
if command -v system76-power &> /dev/null; then
    echo "   system76-power is installed"
    system76-power --help | head -5
else
    echo "   system76-power not found"
fi

echo ""
echo "✅ Diagnostic complete!"
echo ""
echo "💡 If PWM keeps getting reset to 0:"
echo "   1. Check if system76-power daemon is running"
echo "   2. Try stopping system76-power: sudo systemctl stop system76-power"
echo "   3. Check for other fan control software"
echo "   4. Verify the fan is physically connected"
