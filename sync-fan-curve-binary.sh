#!/bin/bash

# Sync Fan Curve App binary between systems via removable media.
# Usage:
#   On source (working system):
#     ./sync-fan-curve-binary.sh export /media/system76/USB_DRIVE
#   On target (Mega):
#     ./sync-fan-curve-binary.sh install /media/system76/USB_DRIVE
#
# Optional direct file path install on target:
#     ./sync-fan-curve-binary.sh install /path/to/fan-curve-app

set -euo pipefail

SCRIPT_NAME=$(basename "$0")
MODE=${1:-}
SRC_OR_DEST=${2:-}

SOURCE_BIN="/usr/local/bin/fan-curve-app"
PACKAGE_DIR_NAME="fan-curve-app-bundle"
TIMESTAMP=$(date +"%Y%m%d-%H%M%S")

print_usage() {
  cat <<EOF
${SCRIPT_NAME} - Export/install fan-curve-app binary via USB or file

Usage:
  Export to USB (run on source system):
    ${SCRIPT_NAME} export /media/system76/USB_DRIVE

  Install from USB (run on target system):
    ${SCRIPT_NAME} install /media/system76/USB_DRIVE

  Install from explicit binary path (run on target system):
    ${SCRIPT_NAME} install /path/to/fan-curve-app
EOF
}

err() { echo "[ERROR] $*" >&2; exit 1; }
info() { echo "[INFO]  $*"; }
ok()   { echo "[OK]    $*"; }

require_path_exists() {
  local p="$1"
  [ -e "$p" ] || err "Path not found: $p"
}

export_to_usb() {
  local usb_root="$1"
  require_path_exists "$usb_root"

  require_path_exists "$SOURCE_BIN"

  local dest_dir="$usb_root/${PACKAGE_DIR_NAME}-${TIMESTAMP}"
  mkdir -p "$dest_dir"

  info "Copying binary to: $dest_dir/"
  cp -f "$SOURCE_BIN" "$dest_dir/fan-curve-app"
  chmod 755 "$dest_dir/fan-curve-app"

  info "Writing metadata and checksums"
  (
    echo "source_host=$(hostname)"
    echo "timestamp=$(date --iso-8601=seconds)"
    echo "source_path=$SOURCE_BIN"
  ) > "$dest_dir/metadata.txt"

  ( cd "$dest_dir" && md5sum fan-curve-app > MD5SUMS.txt )

  ok "Export complete"
  echo "Bundle: $dest_dir"
}

install_from_bundle_dir() {
  local bundle_dir="$1"
  require_path_exists "$bundle_dir/fan-curve-app"

  info "Verifying checksum in bundle"
  if [ -f "$bundle_dir/MD5SUMS.txt" ]; then
    ( cd "$bundle_dir" && md5sum -c MD5SUMS.txt ) || err "Checksum verification failed"
  else
    info "MD5SUMS.txt not found; proceeding without bundle verification"
  fi

  local target="/usr/local/bin/fan-curve-app"
  local backup="/usr/local/bin/fan-curve-app.bak-${TIMESTAMP}"

  if [ -f "$target" ]; then
    info "Backing up existing binary to $backup"
    sudo cp -f "$target" "$backup"
  fi

  info "Installing new binary to $target"
  sudo cp -f "$bundle_dir/fan-curve-app" "$target"
  sudo chown root:root "$target"
  sudo chmod 755 "$target"

  info "Post-install checksum of installed binary:"
  md5sum "$target" | awk '{print "  " $0}'

  ok "Install complete"
}

install_from_usb_root() {
  local usb_root="$1"
  require_path_exists "$usb_root"

  # Find the newest bundle directory
  local latest
  latest=$(ls -1dt "$usb_root"/${PACKAGE_DIR_NAME}-*/ 2>/dev/null | head -n1 || true)
  [ -n "${latest}" ] || err "No ${PACKAGE_DIR_NAME}-* bundle directories found in $usb_root"

  info "Found bundle: $latest"
  install_from_bundle_dir "$latest"
}

install_from_file() {
  local file_path="$1"
  require_path_exists "$file_path"

  local target="/usr/local/bin/fan-curve-app"
  local backup="/usr/local/bin/fan-curve-app.bak-${TIMESTAMP}"

  if [ -f "$target" ]; then
    info "Backing up existing binary to $backup"
    sudo cp -f "$target" "$backup"
  fi

  info "Installing new binary from $file_path to $target"
  sudo cp -f "$file_path" "$target"
  sudo chown root:root "$target"
  sudo chmod 755 "$target"

  info "Post-install checksum of installed binary:"
  md5sum "$target" | awk '{print "  " $0}'

  ok "Install complete"
}

main() {
  case "$MODE" in
    export)
      [ -n "${SRC_OR_DEST}" ] || { print_usage; err "Missing USB mount path for export"; }
      export_to_usb "$SRC_OR_DEST"
      ;;
    install)
      [ -n "${SRC_OR_DEST}" ] || { print_usage; err "Missing USB mount path or file path for install"; }
      if [ -d "$SRC_OR_DEST" ]; then
        install_from_usb_root "$SRC_OR_DEST"
      else
        install_from_file "$SRC_OR_DEST"
      fi
      ;;
    ""|help|-h|--help)
      print_usage
      ;;
    *)
      print_usage
      err "Unknown mode: $MODE"
      ;;
  esac
}

main "$@"


