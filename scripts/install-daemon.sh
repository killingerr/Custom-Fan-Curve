#!/bin/bash
# Install and start the fan-curve daemon (root-owned DBus service).
# Run from the repo root: sudo ./scripts/install-daemon.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_SRC="$ROOT/target/release/fan-curve-app"
INSTALL_BIN="/usr/local/bin/fan-curve-app"

if [[ $EUID -ne 0 ]]; then
  echo "Run as root: sudo $0"
  exit 1
fi

if [[ ! -x "$BIN_SRC" ]]; then
  echo "Missing release binary. Build first:"
  echo "  cargo build --release"
  exit 1
fi

echo "Installing binary..."
install -m 755 "$BIN_SRC" "$INSTALL_BIN"
ln -sfn "$INSTALL_BIN" /usr/local/bin/fan-curve

echo "Installing DBus policy..."
install -m 644 "$ROOT/data/com.system76.FanCurveDaemon.conf" \
  /usr/share/dbus-1/system.d/com.system76.FanCurveDaemon.conf

echo "Installing systemd unit..."
install -m 644 "$ROOT/data/fan-curve-daemon.service" \
  /etc/systemd/system/fan-curve-daemon.service

echo "Seeding system config..."
# Copy an existing user config; otherwise the daemon creates its own default
# with system76-power's built-in curves on first start.
mkdir -p /etc/fan-curve-app
if [[ ! -f /etc/fan-curve-app/config.json ]]; then
  if [[ -f "$HOME/.fan_curve_app/config.json" ]]; then
    cp "$HOME/.fan_curve_app/config.json" /etc/fan-curve-app/config.json
  elif [[ -f /home/${SUDO_USER:-}/.fan_curve_app/config.json ]]; then
    cp "/home/${SUDO_USER}/.fan_curve_app/config.json" /etc/fan-curve-app/config.json
  fi
fi

systemctl daemon-reload
# Reload DBus config if possible (policy may need a bus restart on some systems)
if command -v systemctl >/dev/null; then
  systemctl reload dbus 2>/dev/null || systemctl reload dbus.service 2>/dev/null || true
fi

systemctl enable fan-curve-daemon.service
# Restart so an update runs the new binary; enable --now leaves an already
# running daemon on the old one.
systemctl restart fan-curve-daemon.service

check_system76_power() {
  # system76-power only drives the fans on Thelio Io boards, so a version
  # without ClaimFanControl only conflicts with the daemon there.
  if ! grep -qsx -e system76_thelio_io -e system76_io /sys/class/hwmon/*/name; then
    return 0
  fi
  if ! busctl status com.system76.PowerDaemon > /dev/null 2>&1; then
    return 0
  fi
  if ! busctl introspect com.system76.PowerDaemon /com/system76/PowerDaemon \
      com.system76.PowerDaemon 2> /dev/null | grep -q ClaimFanControl; then
    echo "WARNING: this system76-power can't hand over fan control (no ClaimFanControl)."
    echo "Its fan curve and this app's will both set the fans until it is updated."
  fi
}
check_system76_power

echo ""
if systemctl is-active --quiet fan-curve-daemon.service; then
  echo "fan-curve-daemon is running."
  echo "Check:  systemctl status fan-curve-daemon"
  echo "Logs:   journalctl -u fan-curve-daemon -f"
  echo "Status: fan-curve fan-curve status"
  echo ""
  echo "Desktop notifications (overheating, fan control problems) need the agent"
  echo "running in your session: run ./install.sh, or start 'fan-curve-app notify'."
else
  echo "Daemon failed to start. Check: journalctl -u fan-curve-daemon -xe"
  exit 1
fi
