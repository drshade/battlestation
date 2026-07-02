#!/usr/bin/env sh
# Report THIS Claude Code instance's status for the Noctalia widget. State is
# per-session: file name = session id, content (tab-separated) =
#   "<wsid>\t<status>\t<kind>\t<title>".
# `kind` identifies the agent (claude here; the widget is ready for codex/gemini/
# ... writing the same format with their own kind). `title` is the session's
# current aiTitle, shown in the bot's hover tooltip.
# The hook event JSON arrives on stdin (we read session_id + transcript_path).
#
# Usage: claude-ws-status.sh <green|purple|orange|clear>
#   green = thinking   purple = running a tool   orange = waiting   clear = ended

kind="claude"
status="${1:-}"
[ -n "$status" ] || exit 0
dir="${XDG_RUNTIME_DIR:-/tmp}/claude-ws"
mkdir -p "$dir"

input=$(cat 2>/dev/null)
meta=$(printf '%s' "$input" | python3 -c '
import sys, json
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
print(d.get("session_id", "") or "")
print(d.get("transcript_path", "") or "")
' 2>/dev/null)
sid=$(printf '%s\n' "$meta" | sed -n 1p)
tpath=$(printf '%s\n' "$meta" | sed -n 2p)
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

# Current session title = the last "ai-title" record in the transcript (Claude
# rewrites it as the conversation evolves). grep is a single fast pass; the tiny
# python just pulls aiTitle out of that one line and collapses any whitespace so
# it can't break the tab-separated record. Empty until Claude generates a title.
title=""
if [ -n "$tpath" ] && [ -f "$tpath" ]; then
  title=$(grep '"type":"ai-title"' "$tpath" 2>/dev/null | tail -n1 | python3 -c '
import sys, json
try:
    print(" ".join((json.loads(sys.stdin.readline() or "{}").get("aiTitle", "") or "").split()))
except Exception:
    print("")
' 2>/dev/null)
fi

printf '%s\t%s\t%s\t%s' "$ws" "$status" "$kind" "$title" > "$dir/$sid"
