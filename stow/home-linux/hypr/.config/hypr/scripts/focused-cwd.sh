#!/usr/bin/env bash
# focused-cwd.sh — print the working directory of the focused *terminal* window.
#
# Hyprland reports the focused window's PID, which for a terminal is the emulator
# (e.g. kitty), not the shell. We confirm the window is a terminal, then descend
# the process tree to the foreground process and read its /proc/<pid>/cwd. kitty
# spawns short-lived `kitten` helpers that sit at $HOME, so we skip kitty/kitten
# when choosing where to descend.
#
# Prints the dir on stdout, or NOTHING when the focused window isn't a terminal
# or no cwd resolves (GUI apps don't track their view in the process cwd) — the
# caller decides the fallback. Shared by term-here.sh and code-here.sh.
set -euo pipefail

# Terminal window classes — mirrors the swallow/terminal lists in misc.lua and
# windowrules.lua. Keep in sync if you add a terminal there.
term_re='^(kitty|ghostty|[Kk]onsole|Alacritty|gnome-terminal|xfce[0-9]?-terminal)$'

# "Anchor" GUI apps we launch with a meaningful cwd (code-here.sh cds VS Code to
# the project root). Unlike terminals we read the window process's OWN cwd — its
# children are unrelated Electron helpers with cwds of their own.
anchor_re='^code-oss$'

info=$(hyprctl activewindow -j)
class=$(jq -r '.class // empty' <<<"$info")
pid=$(jq -r '.pid // empty' <<<"$info")

[[ -n $pid && $pid -gt 0 ]] || exit 0

if [[ $class =~ $term_re ]]; then
    # Descend to the deepest non-helper descendant: kitty -> fish -> (program).
    target=$pid
    while :; do
        # `|| true`: at the leaf, pgrep finds no children and exits non-zero;
        # without it, pipefail + set -e would abort before we print anything.
        child=$(pgrep -P "$target" 2>/dev/null | while read -r c; do
            case "$(cat "/proc/$c/comm" 2>/dev/null)" in
                kitty | kitten) ;;        # skip kitty's own helper processes
                *) echo "$c" ;;
            esac
        done | tail -n1) || true
        [[ -z $child ]] && break
        target=$child
    done
elif [[ $class =~ $anchor_re ]]; then
    target=$pid
else
    exit 0
fi

cwd=$(readlink -f "/proc/$target/cwd" 2>/dev/null || true)
[[ -d $cwd ]] && printf '%s\n' "$cwd"
