#!/usr/bin/env sh
# Fetch Claude Code plan usage from the OAuth usage endpoint (same data as
# /usage) and emit a compact JSON line for the Noctalia widget. The OAuth token
# is read from ~/.claude/.credentials.json and never leaves this script.
#
# The reading is cached so the per-monitor pollers share a single API call and a
# rate-limited (429) or expired-token (401) request never clobbers a good value:
# on anything but a clean 200 the script prints nothing and the widget keeps its
# current numbers.

cache="${XDG_CACHE_HOME:-$HOME/.cache}/claude-usage.json"
lock="$cache.lock"
ttl=240

fresh() {
  [ -f "$cache" ] || return 1
  [ "$(( $(date +%s) - $(stat -c %Y "$cache" 2>/dev/null || echo 0) ))" -lt "$ttl" ]
}

# Fast path: a recent reading already exists, so don't touch the network.
if fresh; then
  cat "$cache"
  exit 0
fi

# Serialize refreshes across monitors; a loser waits and serves the winner's result.
exec 9>"$lock"
if ! flock -n 9; then
  flock 9
  fresh && cat "$cache"
  exit 0
fi
# Won the lock — re-check in case the previous holder just refreshed.
if fresh; then
  cat "$cache"
  exit 0
fi

tok=$(python3 -c "import json; print(json.load(open('$HOME/.claude/.credentials.json'))['claudeAiOauth']['accessToken'])" 2>/dev/null)
[ -n "$tok" ] || exit 0

resp=$(curl -s -w '\n%{http_code}' --max-time 6 https://api.anthropic.com/api/oauth/usage \
  -H "Authorization: Bearer $tok" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -H "anthropic-version: 2023-06-01" 2>/dev/null)
[ "$(printf '%s' "$resp" | tail -n1)" = "200" ] || exit 0

out=$(printf '%s' "$resp" | sed '$d' | python3 -c "
import sys, json
d = json.load(sys.stdin)
fh = d.get('five_hour')
sd = d.get('seven_day')
if not isinstance(fh, dict) or not isinstance(sd, dict):
    sys.exit(1)
print(json.dumps({
    'sessionPct':    round(fh.get('utilization') or 0),
    'sessionResets': fh.get('resets_at') or '',
    'weeklyPct':     round(sd.get('utilization') or 0),
    'weeklyResets':  sd.get('resets_at') or '',
}))
" 2>/dev/null)
[ -n "$out" ] || exit 0

printf '%s\n' "$out" > "$cache.tmp" && mv "$cache.tmp" "$cache"
printf '%s\n' "$out"
