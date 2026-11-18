#!/bin/sh
# presence-report.sh — the macOS counterpart of hypridle's idle listener:
# tells bsctl whether the human is at the desk (`bsctl presence`, contract
# in ctl/src/lib.rs). Run every 15s by the launchd agent beside it in this
# package. Reports EDGES only — it compares the measured state with what
# bsctl already holds, so there is no state of its own to drift (a reboot
# clears bsctl's runtime store; the next run reports afresh).
#
# Idle = no keyboard/mouse input for THRESHOLD seconds (HIDIdleTime, the
# same signal the screensaver uses), matching hypridle's 120s listener.
set -eu

THRESHOLD=120
bsctl="$HOME/.local/bin/bsctl"
[ -x "$bsctl" ] || exit 0

ns=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ { print $NF; exit }')
idle=$(( ${ns:-0} / 1000000000 ))
if [ "$idle" -ge "$THRESHOLD" ]; then want=idle; else want=active; fi

have=$("$bsctl" presence get --format json | sed -n 's/.*"state":"\([a-z]*\)".*/\1/p')
[ "$want" = "$have" ] && exit 0

if [ "$want" = idle ]; then
  "$bsctl" presence set idle --already "$idle"
else
  "$bsctl" presence set active
fi
