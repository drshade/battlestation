#!/usr/bin/env sh
# Home workspaces round-robin across the three docked outputs, left to right,
# instead of blocking them (1-10 on one display, 11-20 on the next, ...).
# Physical arrangement on this machine: DP-2 (left) - eDP-1 (middle) -
# HDMI-A-2 (right) -- port names, stable across the home/office monitor
# profiles (config/profile.lua), so hardcoding them here is safe.
#
# Writes via --ws-id (raw Hyprland id), not --bs-id: bs-id only resolves
# against currently-live workspaces, so it can't pre-declare a home for a
# workspace that doesn't exist yet (see ctl/src/ws.rs resolve_ws_sel). Safe
# to re-run with a larger count to extend the range later.
set -eu

count="${1:-30}"
outputs="DP-2 eDP-1 HDMI-A-2"
bsctl="$HOME/.local/bin/bsctl"

i=1
while [ "$i" -le "$count" ]; do
    idx=$(( (i - 1) % 3 + 1 ))
    output=$(echo "$outputs" | cut -d' ' -f"$idx")
    "$bsctl" ws prefs add --ws-id "$i" --display-name "$output"
    i=$((i + 1))
done

"$bsctl" ws prefs reconcile
