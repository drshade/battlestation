#!/usr/bin/env sh
# Stow every package into $HOME. Every subdirectory of stow/ is a package, by
# construction — there is no ignore-list to maintain. Repo meta (README, AGENTS,
# setup/, this script) lives outside stow/ and is therefore never stowed.
set -eu
cd "$(dirname "$0")/stow"

for d in */; do
  stow --restow --target="$HOME" "${d%/}"
  echo "stowed: ${d%/}"
done
