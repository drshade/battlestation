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

# Toast the new mode, reusing one notification slot so rapid re-toggles replace
# the toast instead of stacking. We persist the daemon-assigned id and feed it
# back via --replace-id (the x-canonical synchronous hint isn't honored here).
case "$next" in
    scrolling) label="Scrolling — columns flow over the edge" ;;
    dwindle)   label="Dwindle — tiling splits" ;;
esac
idfile="${XDG_RUNTIME_DIR:-/tmp}/hypr-layout-notify.id"
prev=$(cat "$idfile" 2>/dev/null || echo 0)
id=$(notify-send -a Hyprland -t 1500 -r "$prev" -p "Layout: $next" "$label" 2>/dev/null) \
    && printf '%s' "$id" > "$idfile" || true
