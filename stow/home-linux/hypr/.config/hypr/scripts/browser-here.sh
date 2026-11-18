#!/usr/bin/env bash
# browser-here.sh — open URL(s) in a browser window ON THE CURRENT WORKSPACE.
#
# Firefox opens a handed-over URL in whichever of its windows was most recently
# focused, with no notion of workspaces; with one Firefox window per workspace
# a link clicked in a terminal lands in some other workspace's window (and
# misc:focus_on_activate is off, so you aren't even taken there). This wrapper
# is the default browser (browser-here.desktop) so every xdg-open goes through
# it, and HYPER+B uses it too:
#   * a Firefox window exists on the active workspace -> focus it (the most
#     recently used one if several), then hand Firefox the URL, which now opens
#     as a tab in THAT window. No URL -> just focus it.
#   * none there -> `firefox --new-window` tiles a fresh window here.
#
# Usage: browser-here.sh [url...]
set -euo pipefail

browser=firefox
class=firefox

ws=$(hyprctl activeworkspace -j | jq -r '.id')
addr=$(hyprctl clients -j | jq -r --argjson ws "$ws" \
    '[.[] | select(.class == "'"$class"'" and .mapped and .workspace.id == $ws)]
     | sort_by(.focusHistoryID) | .[0].address // empty')

if [[ -n $addr ]]; then
    # Lua-config build: dispatchers are Lua expressions, not "focuswindow address:...".
    hyprctl dispatch "hl.dsp.focus({ window = \"address:$addr\" })" >/dev/null
    [[ $# -eq 0 ]] && exit 0
    # Give Firefox a moment to see the focus change before it picks a window.
    sleep 0.15
    exec "$browser" "$@"
fi

exec "$browser" --new-window "$@"
