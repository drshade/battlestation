#!/usr/bin/env sh
# Stow the root groups into /: stow/root/ (every OS) plus stow/root-<os>/
# for the running OS (see stow/lib.sh).
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "stow-root: must be run as root. Try: sudo $0" >&2
  exit 1
fi

STOW_DIR=$(cd "$(dirname "$0")" && pwd -P)
# shellcheck source=stow/lib.sh
. "$STOW_DIR/lib.sh"

groups=$(stow_groups_for root)
if [ -z "$groups" ]; then
  echo "stow-root: no root group for $(stow_os) — nothing to stow."
  exit 0
fi
for group in $groups; do
  stow_group "$group" /
done
