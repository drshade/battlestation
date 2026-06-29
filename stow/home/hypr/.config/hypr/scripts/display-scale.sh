#!/usr/bin/env sh
# Rescale the focused monitor at runtime — for demos where the audience needs a
# bigger picture. We re-issue the whole monitor line, preserving the current
# mode (so e.g. the ultrawide keeps its local-override resolution instead of
# falling back to "highrr") and changing only the scale. This relayouts the
# workspace — expected, fine for a demo. Bigger scale = larger UI = "zoomed in".
#
# Three non-obvious things, all handled below:
#   1. This Hyprland uses the Lua (non-legacy) config parser, so `hyprctl
#      keyword` is a silent no-op ("keyword can't work with non-legacy
#      parsers") — same trap as rename-workspace.sh. We drive the Lua API
#      (hl.monitor) via `hyprctl eval` instead.
#   2. Hyprland only accepts scales on its 1/120 grid that divide the mode into
#      ~whole pixels, and snaps anything else to its own choice. The achievable
#      scales are irregular per panel, so we don't compute them — we step a
#      fixed LADDER of target scales and let Hyprland snap each rung. On
#      3440x1440 these seven rungs land on seven distinct scales (no dead step).
#   3. We persist the ladder INDEX per-monitor, so stepping is deterministic and
#      immune to how Hyprland rounds the reported scale. With no saved index
#      (first use / after reset) we start from the rung nearest the live scale,
#      so a HiDPI default (auto > 1.0) is honoured.
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
