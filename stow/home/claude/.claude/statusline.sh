#!/usr/bin/env bash
# Claude Code status line.
# Shows: dir (git branch) · context% · session/weekly rate-limit usage.
# Rate-limit fields (rate_limits.*) are only present for Pro/Max subscribers
# and only after the first API response, so we degrade gracefully when absent.

export LC_ALL=C   # decimal point, not comma, so printf/awk parse floats

input=$(cat)

# --- ANSI helpers -----------------------------------------------------------
dim=$'\e[2m'; reset=$'\e[0m'
cyan=$'\e[36m'; magenta=$'\e[35m'
green=$'\e[32m'; yellow=$'\e[33m'; red=$'\e[31m'

# Color a percentage by how much headroom is left: green <50, yellow <80, red.
pct_color() { # $1 = numeric percent
  awk -v p="$1" -v g="$green" -v y="$yellow" -v r="$red" 'BEGIN{
    if (p+0 >= 80) printf r; else if (p+0 >= 50) printf y; else printf g }'
}

# --- pull fields (newline-separated so empty fields are preserved) ----------
mapfile -t f < <(
  printf '%s' "$input" | jq -r '
    ((.workspace.current_dir // .cwd // "") | split("/") | last // ""),
    (.workspace.git_worktree // ""),
    (.context_window.used_percentage // ""),
    (.rate_limits.five_hour.used_percentage // ""),
    (.rate_limits.seven_day.used_percentage // "")'
)
dir=${f[0]}; branch=${f[1]}; ctx=${f[2]}; session=${f[3]}; weekly=${f[4]}

out="${cyan}${dir}${reset}"
[ -n "$branch" ] && out+=" ${magenta}${branch}${reset}"

# Context window usage
if [ -n "$ctx" ]; then
  out+="  ${dim}·${reset} ctx $(pct_color "$ctx")$(printf '%.0f' "$ctx")%${reset}"
fi

# Session (5h) / weekly (7d) rate-limit usage — the requested bit.
if [ -n "$session" ] || [ -n "$weekly" ]; then
  s="${session:-0}"; w="${weekly:-0}"
  out+="  ${dim}·${reset} $(pct_color "$s")$(printf '%.0f' "$s")%${reset}"
  out+=" ${dim}/${reset} $(pct_color "$w")$(printf '%.0f' "$w")%${reset}"
  out+=" ${dim}(session / weekly)${reset}"
fi

printf '%b' "$out"
