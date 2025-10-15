#!/usr/bin/env bash
set -euo pipefail

# Monitor system76-power daemon logs and pwm1 value side-by-side.
# - Dynamically detects system76_thelio_io hwmon path (can be overridden).
# - Samples pwm1 every 0.5s with timestamps.
# - Tails daemon journal logs.
#
# Usage:
#   bash monitor-daemon-and-pwm.sh                # auto-detect hwmon
#   HWMON_PATH=/sys/class/hwmon/hwmon5 bash monitor-daemon-and-pwm.sh
#   PWM_INDEX=1 bash monitor-daemon-and-pwm.sh

UNIT="com.system76.PowerDaemon.service"
PWM_INDEX="${PWM_INDEX:-1}"

detect_hwmon() {
  if [[ -n "${HWMON_PATH:-}" && -d "$HWMON_PATH" ]]; then
    echo "$HWMON_PATH"
    return 0
  fi
  local line
  line=$(grep -H . /sys/class/hwmon/*/name 2>/dev/null | grep system76_thelio_io || true)
  if [[ -z "$line" ]]; then
    echo ""; return 1
  fi
  local dir
  dir=$(echo "$line" | head -n1 | awk -F'/' '{print $(NF-1)}')
  echo "/sys/class/hwmon/$dir"
}

HWMON_PATH=$(detect_hwmon || true)
if [[ -z "$HWMON_PATH" ]]; then
  echo "[ERROR] Could not detect system76_thelio_io hwmon. Dumping available names:" >&2
  grep -H . /sys/class/hwmon/*/name 2>/dev/null || true
  exit 1
fi

PWM_FILE="$HWMON_PATH/pwm$PWM_INDEX"
if [[ ! -f "$PWM_FILE" ]]; then
  echo "[ERROR] PWM file not found: $PWM_FILE" >&2
  exit 1
fi

TS=$(date +%Y%m%d-%H%M%S)
OUT_DIR="$(pwd)/monitor-$TS"
mkdir -p "$OUT_DIR"

echo "[INFO] Using hwmon: $HWMON_PATH (pwm$PWM_INDEX)" | tee -a "$OUT_DIR/monitor.log"
echo "[INFO] Logs directory: $OUT_DIR" | tee -a "$OUT_DIR/monitor.log"

# Ensure background processes are stopped on exit
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT INT TERM

echo "[INFO] Tailing daemon logs (journalctl -u $UNIT -f) -> $OUT_DIR/daemon.tail.log"
journalctl -u "$UNIT" -f --no-pager | tee "$OUT_DIR/daemon.tail.log" &
PIDS+=($!)

echo "[INFO] Sampling $PWM_FILE every 0.5s -> $OUT_DIR/pwm1.watch.log"
(
  while true; do
    now=$(date +"%Y-%m-%d %H:%M:%S")
    val=$(cat "$PWM_FILE" 2>/dev/null || echo "ERR")
    echo "$now pwm$PWM_INDEX=$val"
    sleep 0.5
  done
) | tee "$OUT_DIR/pwm1.watch.log" &
PIDS+=($!)

echo "[INFO] Press Ctrl-C to stop. Files will remain in $OUT_DIR"
wait


