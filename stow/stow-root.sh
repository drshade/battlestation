#!/usr/bin/env sh
# Stow every package in stow/root/ into / (system configs, e.g. /etc/...).
# Must be run as root:  sudo ./stow/stow-root.sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "stow-root: must be run as root. Try: sudo $0" >&2
  exit 1
fi

dir="$(dirname "$0")/root"
if [ ! -d "$dir" ]; then
  echo "stow-root: no stow/root/ yet — nothing to stow."
  exit 0
fi
cd "$dir"

for pkg in */; do
  [ -d "$pkg" ] || continue          # skip if root/ is empty
  name="${pkg%/}"
  stow --restow --target="/" "$name"
  echo "stowed: root/$name -> /"
done
