#!/usr/bin/env sh
# Enable/disable the internal laptop panel in step with the lid.
#
# The internal panel is found by connector class -- eDP/LVDS/DSI are built-in
# panels, DP/HDMI/etc. are external -- so this needs no per-machine output name
# and is inert on desktops: no internal panel is matched, and there's no Lid
# Switch to call it. ACPI exposes no lid->display link (only open/closed state),
# so the panel is identified this way rather than from the lid itself.
#
# Like display-scale.sh, monitor changes go through `hyprctl eval` / the Lua API,
# since the Lua (non-legacy) config parser makes `hyprctl keyword` a no-op.
#
# Usage:
#   clamshell.sh on      disable the internal panel (lid closed)
#   clamshell.sh off     enable  the internal panel (lid open)
#   clamshell.sh auto    disable only if the lid currently reads closed (startup)
set -eu

mode="${1:-auto}"

lid_closed() {
    for s in /proc/acpi/button/lid/*/state; do
        [ -e "$s" ] || return 1          # no lid file -> desktop
        grep -q closed "$s" && return 0
    done
    return 1
}

# Startup: only act when the lid is already shut; otherwise leave panels alone.
if [ "$mode" = "auto" ]; then
    lid_closed || exit 0
    mode="on"
fi

case "$mode" in
    on)  disabled=true  ;;
    off) disabled=false ;;
    *)   echo "usage: clamshell.sh on|off|auto" >&2; exit 2 ;;
esac

# `monitors all` so a panel that is currently disabled is still listed when we
# turn it back on. Usually one match; loop handles dual internal panels.
hyprctl monitors all -j | jq -r '.[].name | select(test("^(eDP|LVDS|DSI)"))' |
while IFS= read -r out; do
    [ -n "$out" ] || continue
    hyprctl eval "hl.monitor({output='$out', disabled=$disabled})"
done
