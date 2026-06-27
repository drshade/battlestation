#!/usr/bin/env sh
# Rename the active Hyprland workspace via a zenity prompt.
# This Hyprland uses the Lua config parser, so `hyprctl dispatch renameworkspace`
# fails — the rename must go through hl.dsp.workspace.rename instead.
set -eu

ws=$(hyprctl activeworkspace -j)
id=$(printf '%s' "$ws" | python3 -c 'import sys,json; print(json.load(sys.stdin)["id"])')
current=$(printf '%s' "$ws" | python3 -c 'import sys,json; print(json.load(sys.stdin)["name"])')

name=$(zenity --entry --title "Rename workspace" --text "Name for workspace $id:" --entry-text "$current") || exit 0
[ -z "$name" ] && name="$id"                     # empty -> reset to the number
name=$(printf '%s' "$name" | sed 's/[\\"]/\\&/g') # escape \ and " for the Lua string

hyprctl dispatch "hl.dsp.workspace.rename({ workspace = $id, name = \"$name\" })"
