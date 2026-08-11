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
mkdir -p /etc/fan-curve-app
if [[ ! -f /etc/fan-curve-app/config.json ]]; then
  if [[ -f "$HOME/.fan_curve_app/config.json" ]]; then
    cp "$HOME/.fan_curve_app/config.json" /etc/fan-curve-app/config.json
  elif [[ -f /home/${SUDO_USER:-}/.fan_curve_app/config.json ]]; then
    cp "/home/${SUDO_USER}/.fan_curve_app/config.json" /etc/fan-curve-app/config.json
  else
    # Minimal Standard default
    cat > /etc/fan-curve-app/config.json << 'EOF'
{
  "curves": [
    {
      "name": "Standard",
      "points": [
        {"temp": 0, "duty": 0},
        {"temp": 30, "duty": 2000},
        {"temp": 40, "duty": 3000},
        {"temp": 50, "duty": 4000},
        {"temp": 60, "duty": 5000},
        {"temp": 70, "duty": 6000},
        {"temp": 80, "duty": 7000},
        {"temp": 90, "duty": 8000},
        {"temp": 100, "duty": 10000}
      ]
    }
  ],
  "default_curve_index": 0
}
EOF
  fi
fi

systemctl daemon-reload
# Reload DBus config if possible (policy may need a bus restart on some systems)
if command -v systemctl >/dev/null; then
  systemctl reload dbus 2>/dev/null || systemctl reload dbus.service 2>/dev/null || true
fi

systemctl enable --now fan-curve-daemon.service

echo ""
if systemctl is-active --quiet fan-curve-daemon.service; then
  echo "fan-curve-daemon is running."
  echo "Check:  systemctl status fan-curve-daemon"
  echo "Logs:   journalctl -u fan-curve-daemon -f"
  echo "Status: fan-curve fan-curve status"
else
  echo "Daemon failed to start. Check: journalctl -u fan-curve-daemon -xe"
  exit 1
fi
