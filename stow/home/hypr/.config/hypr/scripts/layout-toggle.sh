#!/usr/bin/env sh
# Toggle general:layout between dwindle (default BSP tiling) and scrolling
# (native column "flow" layout — columns scroll off the monitor edges).
#
# Like display-scale.sh / clamshell.sh, the runtime change goes through the Lua
# API via `hyprctl eval`, since the Lua (non-legacy) config parser makes
# `hyprctl keyword` a silent no-op.
#
# `scrolling` is built into Hyprland (>=0.55, no plugin); see
# https://wiki.hypr.land/Configuring/Layouts/Scrolling-Layout/
set -eu

cur=$(hyprctl getoption -j general:layout \
    | python3 -c 'import sys, json; print(json.load(sys.stdin)["str"])')

case "$cur" in
    scrolling) next=dwindle ;;
    *)         next=scrolling ;;
esac

hyprctl eval "hl.config({ general = { layout = \"$next\" } })" >/dev/null

# Announce the new mode through the shell's own notification channel (no
# D-Bus round trip; the shell is the notification daemon anyway). Best-effort:
# the layout has already changed, a missing shell must not fail the toggle.
noctalia msg notification-show "Layout: $next" "" >/dev/null 2>&1 || true
