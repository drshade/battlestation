#!/usr/bin/env sh
# Report THIS Claude Code instance's status for the Noctalia widget. State is
# per-session: file name = session id, content = "<workspace-id> <status>".
# The hook event JSON arrives on stdin (we read session_id from it).
#
# Usage: claude-ws-status.sh <green|purple|orange|clear>
#   green = thinking   purple = running a tool   orange = waiting   clear = ended

status="${1:-}"
[ -n "$status" ] || exit 0
dir="${XDG_RUNTIME_DIR:-/tmp}/claude-ws"
mkdir -p "$dir"

input=$(cat 2>/dev/null)
sid=$(printf '%s' "$input" | python3 -c "import sys, json; print(json.load(sys.stdin).get('session_id',''))" 2>/dev/null)
[ -n "$sid" ] || sid="default"

if [ "$status" = "clear" ]; then
  rm -f "$dir/$sid"
  exit 0
fi

# Owning workspace: walk the process tree up to the terminal window.
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
printf '%s %s' "$ws" "$status" > "$dir/$sid"
