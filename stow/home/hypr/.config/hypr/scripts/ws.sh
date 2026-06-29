#!/usr/bin/env sh
# Virtual workspace ordering: navigate/move by DISPLAY POSITION instead of by
# Hyprland's immutable workspace id. A persisted preference list maps position
# <-> real id; the claude-workspaces bar plugin renders the same order and writes
# this file when you drag a pill.
#
# Hyprland has no way to renumber a workspace id, so "reordering" is purely a
# display-layer remap: real ids stay put, positions are what you navigate by.
#
# The order file is just real ids in preferred order, space-separated:
#     3 1 2 5 4 6 7 8 9
# Resolved order = those of them that currently exist (in this order), followed
# by any live workspaces not listed, ascending. Missing file => identity (1,2,3).
#
# Dispatch goes through the Lua API (hl.dsp.*), since the Lua config parser
# rejects legacy `hyprctl dispatch workspace N`. Same reason as display-scale.sh.
#
# Usage:
#   ws.sh goto <pos>                  focus the workspace at display position
#   ws.sh movewindow <pos> [--follow] send active window there (--follow = move to it too)
#   ws.sh relative <next|prev> [--move]  step focus (or window) in display order
#   ws.sh set <id> [id ...]           write the preferred order
#   ws.sh get                         print the raw preference
#   ws.sh order                       print resolved "pos -> id (name)" (debug)
set -eu

ORDER_FILE="${XDG_STATE_HOME:-$HOME/.local/state}/claude-workspaces/order"

# Live, non-special workspace ids (matches what the bar shows), newline-separated.
live_ids() {
    hyprctl workspaces -j | jq -r '.[] | select((.name // "") | startswith("special:") | not) | .id'
}

# Resolved display order: preference (filtered to existing), then newcomers asc.
resolved() {
    pref=""
    [ -f "$ORDER_FILE" ] && pref="$(cat "$ORDER_FILE")"
    live="$(live_ids)"

    for id in $pref; do
        printf '%s\n' "$live" | grep -qxF -- "$id" && printf '%s\n' "$id"
    done
    printf '%s\n' "$live" | sort -n | while IFS= read -r id; do
        [ -n "$id" ] || continue
        case " $pref " in
            *" $id "*) : ;;
            *) printf '%s\n' "$id" ;;
        esac
    done
}

nth()   { resolved | sed -n "${1}p"; }                       # real id at position $1
posof() { resolved | grep -nxF -- "$1" | head -1 | cut -d: -f1; }  # position of real id $1

focus_ws() { hyprctl dispatch "hl.dsp.focus({ workspace = $1 })" >/dev/null; }
move_ws()  { hyprctl dispatch "hl.dsp.window.move({ workspace = $1, follow = $2 })" >/dev/null; }

cmd="${1:-}"
[ -n "$cmd" ] || { echo "usage: ws.sh goto|movewindow|relative|set|get|order ..." >&2; exit 2; }
shift || true

case "$cmd" in
    goto)
        id="$(nth "${1:?position required}")"
        [ -n "$id" ] && focus_ws "$id"
        ;;
    movewindow)
        pos="${1:?position required}"
        follow="false"; [ "${2:-}" = "--follow" ] && follow="true"
        id="$(nth "$pos")"
        [ -n "$id" ] && move_ws "$id" "$follow"
        ;;
    relative)
        case "${1:?next|prev required}" in
            next) d=1 ;;
            prev) d=-1 ;;
            *) echo "relative: expected next|prev" >&2; exit 2 ;;
        esac
        move=""; [ "${2:-}" = "--move" ] && move=1
        cur="$(hyprctl activeworkspace -j | jq .id)"
        pos="$(posof "$cur")"; [ -n "$pos" ] || pos=1
        n="$(resolved | grep -c '')"
        new=$((pos + d))
        [ "$new" -lt 1 ] && new=1
        [ "$new" -gt "$n" ] && new="$n"
        id="$(nth "$new")"
        [ -n "$id" ] || exit 0
        if [ -n "$move" ]; then move_ws "$id" "true"; else focus_ws "$id"; fi
        ;;
    set)
        [ "$#" -gt 0 ] || { echo "set: need at least one id" >&2; exit 2; }
        mkdir -p "$(dirname "$ORDER_FILE")"
        printf '%s\n' "$*" > "$ORDER_FILE"
        ;;
    get)
        cat "$ORDER_FILE" 2>/dev/null || true
        ;;
    order)
        i=1
        resolved | while IFS= read -r id; do
            name="$(hyprctl workspaces -j | jq -r ".[] | select(.id==$id) | .name")"
            printf '%s -> ws %s (%s)\n' "$i" "$id" "$name"
            i=$((i + 1))
        done
        ;;
    *)
        echo "ws.sh: unknown command '$cmd'" >&2; exit 2
        ;;
esac
