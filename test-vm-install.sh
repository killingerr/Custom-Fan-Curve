#!/bin/bash

# VM-based fresh install testing
# Requires VirtualBox or similar VM software

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VM_NAME="fan-curve-test-vm"
VM_IP="192.168.56.100"  # Adjust based on your VM network

print_status() {
    echo "🔧 $1"
}

print_success() {
    echo "✅ $1"
}

print_error() {
    echo "❌ $1"
}

# Check if VM is running
if ! VBoxManage showvminfo "$VM_NAME" --machinereadable | grep -q 'VMState="running"'; then
    print_status "Starting test VM..."
    VBoxManage startvm "$VM_NAME" --type headless
    sleep 30  # Wait for VM to boot
fi

print_status "Testing fresh install on VM..."

# Copy repo to VM
scp -r "$SCRIPT_DIR" testuser@"$VM_IP":/home/testuser/Fan-Curve-App

# Run installation test
ssh testuser@"$VM_IP" << 'EOF'
cd /home/testuser/Fan-Curve-App
chmod +x install.sh
./install.sh
EOF

# Test the installation
if ssh testuser@"$VM_IP" "/usr/local/bin/fan-curve --help" > /dev/null 2>&1; then
    print_success "VM fresh install test PASSED!"
else
    print_error "VM fresh install test FAILED!"
    exit 1
fi

print_success "VM testing completed successfully!"
