#!/usr/bin/env bash
set -euo pipefail

# Diagnostic script for Mega: DBus usage, hwmon path, daemon version, app binaries

TS=$(date +%Y%m%d-%H%M%S)
OUT_DIR="$(pwd)/diagnostics-$TS"
mkdir -p "$OUT_DIR"

log() { echo "[INFO] $*" | tee -a "$OUT_DIR/diagnostics.log"; }
warn() { echo "[WARN] $*" | tee -a "$OUT_DIR/diagnostics.log"; }
err() { echo "[ERROR] $*" | tee -a "$OUT_DIR/diagnostics.log"; }

log "Diagnostics output: $OUT_DIR"

# 1) Verify DBus path is used (not direct sysfs)
log "Checking DBus responsiveness (system76-power Fan.SetDuty)..."
if dbus-send --system --dest=com.system76.PowerDaemon --type=method_call --print-reply \
  /com/system76/PowerDaemon/Fan com.system76.PowerDaemon.Fan.SetDuty byte:255 \
  > "$OUT_DIR/dbus_test.out" 2> "$OUT_DIR/dbus_test.err"; then
  log "DBus call to Fan.SetDuty succeeded (see dbus_test.out)."
else
  warn "DBus call to Fan.SetDuty failed (see dbus_test.err)."
fi

log "Collecting recent daemon logs..."
journalctl -u com.system76.PowerDaemon.service -n 200 --no-pager > "$OUT_DIR/daemon_recent.log" || true

# 2) Check the actual hwmon path and PWM target
log "Detecting system76_thelio_io hwmon path..."
HWMON_LINE=$(grep -H . /sys/class/hwmon/*/name 2>/dev/null | grep system76_thelio_io || true)
if [[ -n "$HWMON_LINE" ]]; then
  echo "$HWMON_LINE" | tee "$OUT_DIR/hwmon_detect.txt"
  HWMON_DIR=$(echo "$HWMON_LINE" | head -n1 | awk -F'/' '{print $(NF-1)}')
  HWMON_PATH="/sys/class/hwmon/$HWMON_DIR"
  log "Detected: $HWMON_PATH"
  for n in 1 2 3 4; do
    if [[ -f "$HWMON_PATH/pwm$n" ]]; then
      log "Reading $HWMON_PATH/pwm$n and pwm${n}_enable..."
      { echo "pwm$n=$(cat "$HWMON_PATH/pwm$n" 2>/dev/null)"; \
        [[ -f "$HWMON_PATH/pwm${n}_enable" ]] && echo "pwm${n}_enable=$(cat "$HWMON_PATH/pwm${n}_enable" 2>/dev/null)" || true; } \
        | tee -a "$OUT_DIR/pwm_state.txt"
    fi
  done
else
  warn "system76_thelio_io hwmon not found. Dumping all hwmon names..."
  grep -H . /sys/class/hwmon/*/name 2>/dev/null | tee "$OUT_DIR/hwmon_all.txt" || true
fi

# 3) Confirm daemon version and fixes are installed
log "Checking system76-power version..."
if command -v system76-power >/dev/null 2>&1; then
  system76-power --version 2>&1 | tee "$OUT_DIR/system76-power.version" || true
else
  warn "system76-power binary not found in PATH."
fi

log "Capturing daemon binary strings (first lines)..."
if [[ -x "/usr/bin/system76-power" ]]; then
  strings /usr/bin/system76-power | head -n 50 > "$OUT_DIR/system76-power.strings.head" || true
fi

# 4) Validate app vs system binary behavior
log "Comparing app binaries..."
SYS_BIN="/usr/local/bin/fan-curve-app"
SYS_ALT_LINK="/usr/local/bin/fan-curve"
LOCAL_BIN="$(pwd)/target/release/fan-curve-app"

for B in "$SYS_BIN" "$LOCAL_BIN"; do
  if [[ -x "$B" ]]; then
    echo "== $B ==" | tee -a "$OUT_DIR/bin_compare.txt"
    ls -la "$B" | tee -a "$OUT_DIR/bin_compare.txt"
    "$B" --version 2>&1 | tee -a "$OUT_DIR/bin_compare.txt" || true
    # Show embedded strings sample
    strings "$B" | head -n 30 >> "$OUT_DIR/bin_compare.txt" || true
  fi
done

if [[ -L "$SYS_ALT_LINK" ]]; then
  log "fan-curve symlink: $(readlink -f "$SYS_ALT_LINK")" | tee -a "$OUT_DIR/bin_compare.txt"
fi

log "Done. Artifacts saved in $OUT_DIR"

