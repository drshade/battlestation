#!/usr/bin/env sh
# Report this Claude Code instance's status into a per-workspace state file, so
# the Noctalia workspace pills can color by Claude status. Last writer wins.
#
# Usage: claude-ws-status.sh <green|purple|orange|clear>
#   green = thinking/processing   purple = running a tool
#   orange = waiting for input    clear  = session ended (remove the marker)

status="${1:-}"
[ -n "$status" ] || exit 0
dir="${XDG_RUNTIME_DIR:-/tmp}/claude-ws"
mkdir -p "$dir"

# The hook runs as a descendant of the terminal window; walk up the process tree
# and match a pid against Hyprland's client list to find the owning workspace.
pids=""
pid=$$
while [ "${pid:-0}" -gt 1 ]; do
  pids="$pids $pid"
  pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
  [ -n "$pid" ] || break
done

ws=$(hyprctl clients -j 2>/dev/null | python3 -c '
import sys, json
pids = set(int(p) for p in sys.argv[1].split())
try:
    for c in json.load(sys.stdin):
        if c.get("pid") in pids:
            print(c["workspace"]["id"]); break
except Exception:
    pass
' "$pids" 2>/dev/null)

[ -n "$ws" ] || exit 0

if [ "$status" = "clear" ]; then
  rm -f "$dir/$ws"
else
  printf '%s' "$status" > "$dir/$ws"
fi
