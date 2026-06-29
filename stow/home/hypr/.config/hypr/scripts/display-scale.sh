#!/usr/bin/env sh
# Step the focused monitor's scale up/down at runtime, preserving its mode.
#
# Two Hyprland quirks this works around:
#   1. The Lua (non-legacy) config parser makes `hyprctl keyword` a silent no-op,
#      so we drive the Lua API (hl.monitor) via `hyprctl eval` instead.
#   2. Hyprland snaps scale to its own 1/120 grid, and the achievable values are
#      irregular per panel — so we step a fixed ladder and let it snap each rung,
#      persisting the rung INDEX per-monitor for deterministic stepping.
#
# Usage: display-scale.sh <up|down|reset>
set -eu

ladder="1.0 1.25 1.5 1.75 2.0 2.5 3.0"   # rung 0 = native; tune freely

action="${1:-}"
[ -n "$action" ] || exit 0

# Focused monitor: name + active mode (WxH@Hz, to preserve it) + reported scale.
read -r name w h rr scale <<EOF
$(hyprctl monitors -j | python3 -c '
import sys, json
for m in json.load(sys.stdin):
    if m.get("focused"):
        print(m["name"], m["width"], m["height"], round(m["refreshRate"]), m["scale"])
        break
')
EOF
[ -n "${name:-}" ] || exit 0

state="${XDG_RUNTIME_DIR:-/tmp}/hypr-display-scale.$name"

if [ "$action" = "reset" ]; then
    rm -f "$state"
    luascale='"auto"'                                   # quoted -> Lua string
else
    saved=$(cat "$state" 2>/dev/null || echo "")
    set -- $(python3 - "$action" "$scale" "$saved" $ladder <<'PY'
import sys
action, scale, saved = sys.argv[1], float(sys.argv[2]), sys.argv[3]
ladder = [float(x) for x in sys.argv[4:]]
idx = int(saved) if saved != "" else min(range(len(ladder)), key=lambda i: abs(ladder[i] - scale))
idx += 1 if action == "up" else -1 if action == "down" else 0
idx = max(0, min(len(ladder) - 1, idx))
print(idx, f"{ladder[idx]:.5f}")
PY
)
    echo "$1" > "$state"                                 # new index
    luascale="$2"                                        # bare -> Lua number
fi

hyprctl eval "hl.monitor({ output = \"$name\", mode = \"${w}x${h}@${rr}\", position = \"auto\", scale = $luascale })" >/dev/null
