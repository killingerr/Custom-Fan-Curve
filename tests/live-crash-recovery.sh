#!/bin/bash
# Crash the installed fan daemon on this machine and check that it recovers:
# the service's stop step hands the fans back, system76-power drops the claim,
# and the daemon restarts and claims the fans again.
#
# Run as your normal user; sudo asks for your password to kill the service:
#   ./tests/live-crash-recovery.sh

set -u

UNIT=fan-curve-daemon.service
failures=0

pass() { echo "  PASS: $*"; }
fail() { echo "  FAIL: $*"; failures=$((failures + 1)); }

fan_control_state() {
    busctl --system call com.system76.FanCurveDaemon /com/system76/FanCurveDaemon \
        com.system76.FanCurveDaemon GetFanControlState 2> /dev/null | cut -d'"' -f2
}

main_pid() {
    systemctl show -p MainPID --value "$UNIT"
}

# pwmN_enable files of System76 fan devices (laptops; Thelio Io 2 has none)
enable_files() {
    for dev in /sys/class/hwmon/hwmon*; do
        case "$(cat "$dev/name" 2> /dev/null)" in
            system76 | system76_acpi | system76_io | system76_thelio_io)
                ls "$dev"/pwm*_enable 2> /dev/null
                ;;
        esac
    done
}

echo "Before the crash"
if ! systemctl is-active --quiet "$UNIT"; then
    echo "  $UNIT is not running; start it first: sudo systemctl start $UNIT"
    exit 1
fi
pid_before=$(main_pid)
state_before=$(fan_control_state)
echo "  daemon PID $pid_before, fan control: $state_before"
mapfile -t enables < <(enable_files)
for f in "${enables[@]}"; do
    echo "  $f = $(cat "$f")"
done
[ ${#enables[@]} -eq 0 ] && echo "  no pwm*_enable files (Thelio Io 2): checking the logs instead"

since=$(date '+%Y-%m-%d %H:%M:%S')
sleep 1

echo
echo "Crashing the daemon (SIGKILL)"
sudo systemctl kill --signal=KILL "$UNIT" || { echo "  could not kill $UNIT"; exit 1; }
# The service restarts 2s after a crash; look before it does
sleep 1

echo
echo "Right after the crash"
if journalctl -u "$UNIT" --since "$since" --no-pager -o cat 2> /dev/null \
        | grep -qE 'Automatic fan control:|No System76 fan controls to restore'; then
    pass "the service ran restore-fans after the crash"
else
    fail "no restore-fans output in the $UNIT log"
fi
for f in "${enables[@]}"; do
    value=$(cat "$f")
    if [ "$value" = 2 ]; then
        pass "$f is back on automatic (2)"
    else
        fail "$f is $value, not automatic (2)"
    fi
done
if [ "$state_before" = claimed ]; then
    if journalctl -u com.system76.PowerDaemon --since "$since" --no-pager -o cat 2> /dev/null \
            | grep -q 'fan control claimant .* disconnected; releasing'; then
        pass "system76-power saw the daemon disappear and released its claim"
    else
        fail "system76-power didn't log releasing the claim"
    fi
fi

echo
echo "After the restart"
pid_after=""
for _ in $(seq 1 20); do
    pid_after=$(main_pid)
    if systemctl is-active --quiet "$UNIT" && [ "$pid_after" != 0 ] && [ "$pid_after" != "$pid_before" ]; then
        break
    fi
    sleep 0.5
done
if [ -n "$pid_after" ] && [ "$pid_after" != 0 ] && [ "$pid_after" != "$pid_before" ]; then
    pass "the daemon restarted (PID $pid_before -> $pid_after)"
else
    fail "the daemon didn't restart"
fi
state_after=""
for _ in $(seq 1 20); do
    state_after=$(fan_control_state)
    [ "$state_after" = "$state_before" ] && break
    sleep 0.5
done
if [ "$state_after" = "$state_before" ]; then
    pass "fan control is '$state_after' again"
else
    fail "fan control is '$state_after', was '$state_before'"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "All checks passed"
else
    echo "$failures check(s) failed"
    exit 1
fi
