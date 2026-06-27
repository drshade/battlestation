#!/usr/bin/env sh
# Fetch Claude Code plan usage from the OAuth usage endpoint (same data as
# /usage) and emit a compact JSON line for the Noctalia widget. The OAuth token
# is read from ~/.claude/.credentials.json and never leaves this script.

tok=$(python3 -c "import json; print(json.load(open('$HOME/.claude/.credentials.json'))['claudeAiOauth']['accessToken'])" 2>/dev/null)
[ -n "$tok" ] || exit 0

curl -s --max-time 6 https://api.anthropic.com/api/oauth/usage \
  -H "Authorization: Bearer $tok" \
  -H "anthropic-beta: oauth-2025-04-20" \
  -H "anthropic-version: 2023-06-01" 2>/dev/null | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    fh = d.get('five_hour') or {}
    sd = d.get('seven_day') or {}
    print(json.dumps({
        'sessionPct':    round(fh.get('utilization') or 0),
        'sessionResets': fh.get('resets_at') or '',
        'weeklyPct':     round(sd.get('utilization') or 0),
        'weeklyResets':  sd.get('resets_at') or '',
    }))
except Exception:
    pass
"
