#!/usr/bin/env bash
# term-here.sh — launch a terminal in the focused window's working directory.
# The directory comes from focused-cwd.sh (shared resolver). When that's empty
# (focused window isn't a terminal), we just launch at the terminal's default.
#
# Usage: term-here.sh <terminal> [args...]   e.g. term-here.sh kitty
set -euo pipefail

here=$(dirname "$(readlink -f "$0")")
term=("$@")
[ ${#term[@]} -eq 0 ] && term=(kitty)

dir=$("$here/focused-cwd.sh" || true)
[[ -n $dir && -d $dir ]] && cd "$dir"

exec "${term[@]}"
