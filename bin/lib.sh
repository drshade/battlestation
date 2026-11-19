# Shared helpers for battlestation repo tooling (bin/*).
# Sourced, never executed directly. Bash.
# shellcheck shell=bash  # no shebang by design; tells shellcheck the dialect (SC2148).

# bash >= 4 is required (associative arrays, mapfile). macOS ships bash 3.2
# as /bin/bash and a PATH can put it first, so rather than depend on PATH
# order, re-exec the calling script under a newer bash when one is
# installed. Sourced files see the caller's $0 and "$@", so this re-runs
# exactly the original command.
if [ "${BASH_VERSINFO[0]:-0}" -lt 4 ]; then
  for _b in /opt/homebrew/bin/bash /usr/local/bin/bash; do
    [ -x "$_b" ] && exec "$_b" "$0" "$@"
  done
  echo "${0##*/}: needs bash >= 4 (this is $BASH_VERSION) — install it: brew install bash" >&2
  exit 1
fi

# Colour only on a real terminal and when not disabled.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_RED=$'\033[31m'; C_GRN=$'\033[32m'; C_YEL=$'\033[33m'
  C_DIM=$'\033[2m';  C_BLD=$'\033[1m';  C_RST=$'\033[0m'
else
  C_RED=; C_GRN=; C_YEL=; C_DIM=; C_BLD=; C_RST=
fi

# Tally, read by the summary at the end of each tool.
FAILS=0
WARNS=0

hdr()  { printf '\n%s== %s ==%s\n' "$C_BLD" "$1" "$C_RST"; }
ok()   { printf '  %s✓%s %s\n' "$C_GRN" "$C_RST" "$1"; }
warn() { printf '  %s⚠%s %s\n' "$C_YEL" "$C_RST" "$1"; WARNS=$((WARNS + 1)); }
bad()  { printf '  %s✗%s %s\n' "$C_RED" "$C_RST" "$1"; FAILS=$((FAILS + 1)); }
info() { printf '  %s%s%s\n' "$C_DIM" "$1" "$C_RST"; }
note() { printf '      %s%s%s\n' "$C_DIM" "$1" "$C_RST"; }

have() { command -v "$1" >/dev/null 2>&1; }

# Iterate the stow groups deployed on THIS machine: prints
# "<group>\t<target-root>" per line on stdout. home -> $HOME, root -> / ;
# an OS-suffixed group (home-linux, root-darwin) maps the same way but is
# listed only on its own OS — the same rule the stow drivers apply
# (stow/lib.sh), so doctor checks exactly what stowing deploys. Skips the
# driver scripts (stow/*.sh). An unknown group (or OS suffix) has no target
# mapping, so it's warned about on stderr and skipped (no stdout line):
# callers machine-parse stdout, and guessing a target root that --fix would
# then restow into is worse than refusing.
stow_groups() {
  local d name base os here
  here=$(uname -s | tr '[:upper:]' '[:lower:]')
  for d in "$REPO"/stow/*/; do
    [ -d "$d" ] || continue
    name=$(basename "$d")
    base=${name%%-*}
    os=${name#"$base"}; os=${os#-}
    case "$os" in
      ""|"$here") ;;
      linux|darwin) continue ;;  # another OS's group: not deployed here
      *) printf "stow_groups: unknown OS suffix in group '%s' (want linux|darwin), skipping\n" "$name" >&2; continue ;;
    esac
    case "$base" in
      home) printf '%s\t%s\n' "$name" "$HOME" ;;
      root) printf '%s\t%s\n' "$name" / ;;
      *)    printf "stow_groups: unknown group '%s' — no target mapping, skipping (add it in bin/lib.sh)\n" "$name" >&2 ;;
    esac
  done
}
