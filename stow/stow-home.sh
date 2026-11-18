#!/usr/bin/env sh
# Stow the home groups into $HOME: stow/home/ (every OS) plus
# stow/home-<os>/ for the running OS (see stow/lib.sh).
set -eu

if [ "$(id -u)" -eq 0 ]; then
  echo "stow-home: refusing to run as root — ~ symlinks must be owned by you." >&2
  echo "           Run it without sudo: ./stow/stow-home.sh" >&2
  exit 1
fi

STOW_DIR=$(cd "$(dirname "$0")" && pwd -P)
# shellcheck source=stow/lib.sh
. "$STOW_DIR/lib.sh"

for group in $(stow_groups_for home); do
  stow_group "$group" "$HOME"
done
