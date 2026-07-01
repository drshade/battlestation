#!/usr/bin/env sh
# Force every display back on. First-line fix for "screens went black".
#
# Why this exists instead of `hyprctl dispatch 'hl.dsp.dpms("on")'`:
# in the Lua (non-legacy) build the dpms dispatcher is TOGGLE-ONLY -- it ignores
# the on/off argument. The string form toggles *every* monitor at once; the table
# form `hl.dsp.dpms({ monitor = "X" })` toggles just that one. Neither can "set
# on". So to turn displays on idempotently we read each output's current
# dpmsStatus and toggle ONLY the ones that are off -- monitors already on are left
# untouched. See AGENTS.md gotchas.
#
# Usage:
#   displays-on.sh          DPMS-on every ENABLED output (fast, idempotent).
#                           Used as hypridle's after_sleep_cmd.
#   displays-on.sh reset    Full recovery: reload config (re-applies monitors.lua
#                           -- native modes, re-enables outputs wrongly disabled,
#                           fixes invalid stored modes), reconcile the lid, then
#                           DPMS-on everything. Use when an output is stuck
#                           disabled or at a bad resolution, not just blanked.
set -eu

dir="$(dirname "$0")"

dpms_on_all() {
    # `monitors` (not `all`) lists only enabled outputs, so a lid-disabled
    # internal panel is correctly left alone. Toggle only outputs that report
    # dpms off (dpms is toggle-only, so toggling an on output would blank it) --
    # this pass only ever turns displays ON, and is idempotent.
    #
    # Bounded retry: right after resume Hyprland's dpms report can be transient
    # (an output may briefly read on while its hardware is still off), so re-read
    # and re-toggle until every enabled output reads on, or we give up. Never
    # blanks an on display -- each pass acts only on outputs currently reading off.
    i=0
    while [ "$i" -lt 10 ]; do
        off=$(hyprctl monitors -j | jq -r '.[] | select(.dpmsStatus == false) | .name')
        [ -n "$off" ] || return 0            # all enabled outputs on -> done
        printf '%s\n' "$off" |
        while IFS= read -r out; do
            [ -n "$out" ] || continue
            hyprctl dispatch "hl.dsp.dpms({ monitor = \"$out\" })" >/dev/null
        done
        i=$((i + 1))
        sleep 0.4
    done
}

case "${1:-}" in
    reset)
        hyprctl reload >/dev/null
        # reload re-enables the internal panel regardless of lid; put it back in
        # step with the actual lid state (no-op on desktops / lid open).
        sh "$dir/clamshell.sh" auto
        dpms_on_all
        ;;
    ""|dpms)
        dpms_on_all
        ;;
    *)
        echo "usage: displays-on.sh [reset]" >&2
        exit 2
        ;;
esac
