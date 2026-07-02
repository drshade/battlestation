#!/usr/bin/env sh
# Report THIS Claude Code instance's status for the Noctalia claude-workspaces
# widget. State lives as small JSON files in one flat dir:
#
#   ${XDG_RUNTIME_DIR:-/tmp}/claude-ws/
#     <session_id>             one file per live session:
#                                {"ws": <int>, "status": "waiting"|"thinking"|"tooling",
#                                 "kind": "claude", "title": "<aiTitle>", "pid": <int>}
#                              `pid` is the Claude Code process (the nearest
#                              ancestor whose comm is `claude`); the widget
#                              sweeps session files whose pid is no longer alive.
#     <session_id>.<agent_id>  one marker per RUNNING subagent:
#                                {"type": "<agent_type>", "description": "<text>"}
#                              Marker mtime = subagent start time. Session ids
#                              are UUIDs and agent ids hex -- neither contains a
#                              dot -- so splitting the file name on the FIRST
#                              dot is unambiguous.
#
# Status is semantic; mapping states to colours is the widget's concern. `kind`
# identifies the agent (claude here; the widget is ready for codex/gemini/...
# writing the same protocol with their own kind). `title` is the session's
# current aiTitle, shown in the bot's hover tooltip.
#
# Usage: claude-ws-status.sh <waiting|thinking|tooling|clear|agent-start|agent-stop>
#   waiting/thinking/tooling  (re)write this session's file with that status
#   clear                     remove the session file AND its subagent markers
#   agent-start               write one subagent marker (fast path: no hyprctl)
#   agent-stop                remove that subagent marker
#
# The hook event JSON arrives on stdin (session_id + transcript_path; agent
# events also carry agent_id, agent_type and possibly description). Debugging:
# set CLAUDE_WS_DEBUG to any non-empty value to append each call's argv + raw
# stdin payload to <dir>/debug.log before processing.

verb="${1:-}"
[ -n "$verb" ] || exit 0
dir="${XDG_RUNTIME_DIR:-/tmp}/claude-ws"
mkdir -p "$dir"

input=$(cat 2>/dev/null)

if [ -n "${CLAUDE_WS_DEBUG:-}" ]; then
  {
    printf '=== %s argv: %s\n' "$(date '+%F %T')" "$*"
    printf '%s\n' "$input"
  } >> "$dir/debug.log"
fi

case "$verb" in
  waiting|thinking|tooling|clear) ;;

  agent-start)
    # Marker content is written by python end-to-end (never hand-concatenated:
    # descriptions may contain quotes). If the payload lacks description or
    # agent_type, fall back to the subagent's meta.json next to the transcript.
    # Temp-file + rename keeps the marker atomic for the widget's poller.
    printf '%s' "$input" | python3 -c '
import sys, os, json
base = sys.argv[1]
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
aid = str(d.get("agent_id") or "")
if not aid:
    sys.exit(0)
sid = str(d.get("session_id") or "") or "default"
atype = str(d.get("agent_type") or "")
desc = str(d.get("description") or "")
if not desc or not atype:
    tp = str(d.get("transcript_path") or "")
    if tp:
        meta = os.path.join(os.path.dirname(tp), sid, "subagents", "agent-" + aid + ".meta.json")
        try:
            with open(meta) as f:
                m = json.load(f)
            desc = desc or str(m.get("description") or "")
            atype = atype or str(m.get("agentType") or "")
        except Exception:
            pass
tmp = os.path.join(base, "." + sid + "." + aid + ".tmp")
with open(tmp, "w") as f:
    json.dump({"type": atype, "description": desc}, f)
os.replace(tmp, os.path.join(base, sid + "." + aid))
' "$dir" 2>/dev/null
    exit 0
    ;;

  agent-stop)
    ids=$(printf '%s' "$input" | python3 -c '
import sys, json
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
print(d.get("session_id", "") or "")
print(d.get("agent_id", "") or "")
' 2>/dev/null)
    sid=$(printf '%s\n' "$ids" | sed -n 1p)
    aid=$(printf '%s\n' "$ids" | sed -n 2p)
    [ -n "$aid" ] || exit 0
    [ -n "$sid" ] || sid="default"
    rm -f "$dir/$sid.$aid"
    exit 0
    ;;

  *) exit 0 ;;
esac

# ---- session status verbs (waiting/thinking/tooling/clear) ------------------

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

if [ "$verb" = "clear" ]; then
  rm -f "$dir/$sid" "$dir/$sid".*
  exit 0
fi

# Walk the process tree up to the terminal window (for the workspace lookup)
# and note the Claude Code process on the way: the nearest ancestor whose comm
# is exactly `claude` (exact match matters -- this script's own comm is the
# 15-char truncation "claude-ws-statu"). Fall back to our immediate parent so
# the pid field is never omitted.
pids=""
claude_pid=""
pid=$$
while [ "${pid:-0}" -gt 1 ]; do
  pids="$pids $pid"
  if [ -z "$claude_pid" ] && [ "$(cat "/proc/$pid/comm" 2>/dev/null)" = "claude" ]; then
    claude_pid="$pid"
  fi
  pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
  [ -n "$pid" ] || break
done
[ -n "$claude_pid" ] || claude_pid="$PPID"

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
# the tooltip stays one line. Empty until Claude generates a title.
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

# JSON is built by python (titles may contain quotes); temp-file + rename keeps
# the session file atomic for the widget's poller.
python3 -c '
import sys, os, json
ws, status, kind, title, pid, path = sys.argv[1:7]
rec = {"ws": int(ws), "status": status, "kind": kind, "title": title, "pid": int(pid)}
tmp = os.path.join(os.path.dirname(path), "." + os.path.basename(path) + ".tmp")
with open(tmp, "w") as f:
    json.dump(rec, f)
os.replace(tmp, path)
' "$ws" "$verb" "claude" "$title" "$claude_pid" "$dir/$sid" 2>/dev/null
