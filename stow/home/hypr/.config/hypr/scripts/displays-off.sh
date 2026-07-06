#!/usr/bin/env sh
# Blank every enabled display (DPMS off). The mirror of displays-on.sh, used by
# hypridle to blank the panels a short while after LOCK.
#
# Why a script instead of `hyprctl dispatch dpms off`: in the Lua (non-legacy)
# build the dpms dispatcher is TOGGLE-ONLY -- it ignores the on/off argument.
# So the only reliable way to reach a known state is to read each output's
# dpmsStatus and toggle ONLY the ones on the wrong side. This pass reads the
# outputs currently ON and toggles them off; outputs already off are left
# untouched, so it is idempotent. See displays-on.sh and AGENTS.md gotchas.
set -eu

# `monitors` (not `all`) lists only ENABLED outputs, so a lid-disabled internal
# panel -- already off -- is correctly left alone. Toggle only outputs reporting
# dpms on; this pass only ever turns displays OFF.
on=$(hyprctl monitors -j | jq -r '.[] | select(.dpmsStatus == true) | .name')
[ -n "$on" ] || exit 0                       # nothing on -> done

printf '%s\n' "$on" |
while IFS= read -r out; do
    [ -n "$out" ] || continue
    hyprctl dispatch "hl.dsp.dpms({ monitor = \"$out\" })" >/dev/null
done
