# Shared helpers for battlestation repo tooling (bin/*).
# Sourced, never executed directly. Bash.
# shellcheck shell=bash  # no shebang by design; tells shellcheck the dialect (SC2148).

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

# Iterate the stow groups: prints "<group>\t<target-root>" per line on stdout.
# home -> $HOME, root -> / . Skips the driver scripts (stow/stow-*.sh).
# An unknown group has no target mapping, so it's warned about on stderr and
# skipped (no stdout line): callers machine-parse stdout, and guessing a target
# root that --fix would then restow into is worse than refusing.
stow_groups() {
  local d name
  for d in "$REPO"/stow/*/; do
    [ -d "$d" ] || continue
    name=$(basename "$d")
    case "$name" in
      home) printf '%s\t%s\n' home "$HOME" ;;
      root) printf '%s\t%s\n' root / ;;
      *)    printf "stow_groups: unknown group '%s' — no target mapping, skipping (add it in bin/lib.sh)\n" "$name" >&2 ;;
    esac
  done
}
