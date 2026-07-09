#!/usr/bin/env bash
# Switch all three monitors' visible workspace together, one group at a
# time. Group N is ws-ids 3N-2/3N-1/3N -- the same left-to-right interleave
# as workspace-display-interleave.sh (DP-2/eDP-1/HDMI-A-2), so a group is
# always one workspace per monitor in physical order.
#
# Ported from ~/repos/dotfiles/hypr/.config/hypr/scripts/workspace-group.sh
# (bound there to SUPER+CTRL+Left/Right): same next/prev/group-number
# stepping logic, but drives bsctl instead of raw hyprctl dispatch, and
# reads the current group from bsctl's own map join instead of `hyprctl
# monitors -j`.
set -euo pipefail

monitors=("DP-2" "eDP-1" "HDMI-A-2")
stride=${#monitors[@]}
action="${1:-next}"
bsctl="$HOME/.local/bin/bsctl"

active_workspace="$("$bsctl" ws map get --format json | jq -r '[.[] | select(.active) | .ws] | min')"

if [[ -z "$active_workspace" || "$active_workspace" == "null" ]]; then
  exit 1
fi

group_base=$((active_workspace - ((active_workspace - 1) % stride)))

case "$action" in
  next)
    target_base=$((group_base + stride))
    ;;
  prev)
    target_base=$((group_base - stride))
    if (( target_base < 1 )); then
      target_base=1
    fi
    ;;
  *)
    if [[ "$action" =~ ^[0-9]+$ && "$action" -gt 0 ]]; then
      target_base=$((1 + ((action - 1) * stride)))
    else
      echo "usage: $0 next|prev|GROUP_NUMBER" >&2
      exit 2
    fi
    ;;
esac

# Focus the monitor first, then the workspace: a not-yet-live ws-id is
# CREATED on focus (bsctl ws focus --help), and creating it lands on
# whichever monitor is currently focused -- so the monitor step must run
# first to land new workspaces on the right display.
for index in "${!monitors[@]}"; do
  monitor="${monitors[$index]}"
  workspace=$((target_base + index))

  "$bsctl" ws focus --display-name "$monitor"
  "$bsctl" ws focus --ws-id "$workspace"
done
