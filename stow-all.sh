#!/usr/bin/env sh
# Stow every package in this repo. Packages are all top-level dirs except the
# meta dirs in $META (docs/runbooks, not configs). The directory list IS the
# source of truth — adding an app needs no edit here.
set -eu
cd "$(dirname "$0")"

META="setup"   # space-separated dirs that are NOT stow packages

for d in */; do
  d="${d%/}"
  case " $META " in *" $d "*) continue ;; esac
  stow --restow --target="$HOME" "$d"
  echo "stowed: $d"
done
