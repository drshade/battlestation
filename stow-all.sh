#!/usr/bin/env sh
# Stow every package into its target root.
#
# Layout: stow/<target-group>/<package>/<mirrored path>
#   - <target-group> names a destination root (see target_for below).
#   - every dir inside a group is a package; there is no ignore-list.
# Repo meta (README, AGENTS, setup/, this script) lives outside stow/ and is
# therefore never stowed.
set -eu
cd "$(dirname "$0")/stow"

# Map each target-group directory to its `stow --target` root.
# To add a new destination (e.g. system configs), add a case here AND create the
# matching stow/<group>/ directory — the two stay in lockstep by construction.
target_for() {
  case "$1" in
    home) printf '%s\n' "$HOME" ;;
    root) printf '%s\n' "/" ;;
    *) echo "stow-all: unknown target group '$1' — add it to target_for()" >&2; exit 1 ;;
  esac
}

for group in */; do
  group="${group%/}"
  target="$(target_for "$group")"
  for pkg in "$group"/*/; do
    [ -d "$pkg" ] || continue          # skip if a group is currently empty
    name="$(basename "$pkg")"
    stow --restow --dir="$group" --target="$target" "$name"
    echo "stowed: $group/$name -> $target"
  done
done
