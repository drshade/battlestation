#!/usr/bin/env sh
# Stow every package in stow/home/ into $HOME.
# Run as your normal user — NOT root (home symlinks must be owned by you).
set -eu

if [ "$(id -u)" -eq 0 ]; then
  echo "stow-home: refusing to run as root — ~ symlinks must be owned by you." >&2
  echo "           Run it without sudo: ./stow/stow-home.sh" >&2
  exit 1
fi

cd "$(dirname "$0")/home"

for pkg in */; do
  [ -d "$pkg" ] || continue          # skip if home/ is somehow empty
  name="${pkg%/}"
  stow --restow --target="$HOME" "$name"
  echo "stowed: home/$name -> $HOME"
done
