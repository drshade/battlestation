#!/usr/bin/env bash
# dolphin-here.sh — open the file manager at the focused terminal's cwd.
# Directory comes from focused-cwd.sh (shared resolver). When that's empty (the
# focused window isn't a terminal), open Dolphin at its default location.
#
# Note: unlike code-here.sh we don't anchor Dolphin's cwd, so this is one-way
# (terminal -> Dolphin). Dolphin doesn't expose its *navigated* folder, and its
# launch cwd would go stale as soon as you browse elsewhere.
set -euo pipefail

here=$(dirname "$(readlink -f "$0")")
dir=$("$here/focused-cwd.sh" || true)

if [[ -n $dir && -d $dir ]]; then
    exec dolphin "$dir"
else
    exec dolphin
fi
