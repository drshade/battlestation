#!/usr/bin/env sh
# Recover a crashed lock screen WITHOUT logging out or rebooting (either would
# kill every running app).
#
# Failure mode: Noctalia — the Quickshell shell that also draws the lock
# surface — crashes while the session is locked. Hyprland keeps the
# ext-session-lock active for security, but with no client drawing the password
# prompt, leaving you on Hyprland's bare "lock app died" recovery screen with no
# way to type a password. The compositor itself is fine; only the locker died.
#
# Hyprland's on-screen recovery hint
#   hyprctl keyword allow_session_lock_restore 1
#   hyprctl dispatch exec hyprlock
# does not work here, for two reasons:
#   1. We run the Lua (non-legacy) config parser, so `hyprctl keyword` and bare
#      `hyprctl dispatch <name>` are rejected — config/dispatch must go through
#      the `hl.*` API (see AGENTS.md "Known gotchas").
#   2. There is no hyprlock; the locker is Noctalia (`qs -c noctalia-shell`).
#
# So this does the Lua-native equivalent: enable lock-restore so a fresh locker
# may adopt the abandoned lock (instead of the protocol killing the new client),
# ensure noctalia-shell is running, then ask it to present the lock surface
# again. You then type your password to unlock normally.
#
# RUN IT FROM ANOTHER VT: the graphical session is locked, so switch to a text
# console (Ctrl+Alt+F3), log in, and run this. It discovers the graphical
# session's Hyprland instance rather than trusting this VT's environment.
# Afterwards switch back (Ctrl+Alt+F1) and enter your password.
set -eu

runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

# --- locate the running Hyprland instance (we are likely on a different VT) ---
if [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
    # shellcheck disable=SC2012  # HIS dirs are machine-named (hash_timestamp) — ls -t is the simple "newest" pick
    his=$(ls -t "$runtime/hypr" 2>/dev/null | head -n1) || true
    [ -n "${his:-}" ] || {
        echo "No Hyprland instance under $runtime/hypr — is Hyprland running?" >&2
        exit 1
    }
    HYPRLAND_INSTANCE_SIGNATURE="$his"
fi
export HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR="$runtime"

# Wayland socket for qs — Hyprland's own environ does not carry WAYLAND_DISPLAY.
if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    for s in "$runtime"/wayland-*; do
        case "$s" in *.lock) continue ;; esac
        [ -S "$s" ] && { WAYLAND_DISPLAY=$(basename "$s"); break; }
    done
fi
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"

# --- sanity: Hyprland reachable? session actually locked? ---
hyprctl version >/dev/null 2>&1 || {
    echo "Can't reach Hyprland (HIS=$HYPRLAND_INSTANCE_SIGNATURE)." >&2
    exit 1
}

if [ "$(hyprctl locked 2>/dev/null)" != "true" ]; then
    echo "Session is not locked — nothing to recover (refusing to lock you out)."
    exit 0
fi

# --- 1. let a replacement locker adopt the abandoned lock ---
hyprctl eval 'hl.config({ misc = { allow_session_lock_restore = true } })' >/dev/null

# --- 2. make sure the shell (and therefore the locker) is running ---
if ! pgrep -f 'qs -c noctalia-shell' >/dev/null 2>&1; then
    echo "noctalia-shell not running — relaunching…"
    # Launch via the compositor so it inherits the graphical env, not this VT's.
    hyprctl dispatch 'hl.dsp.exec_cmd("qs -c noctalia-shell")' >/dev/null
fi

# Wait for the shell's IPC socket to come up (full load takes a few seconds).
pid=""
i=0
while [ "$i" -lt 30 ]; do
    pid=$(pgrep -f 'qs -c noctalia-shell' | head -n1) || true
    if [ -n "$pid" ] && qs ipc --pid "$pid" show >/dev/null 2>&1; then
        break
    fi
    i=$((i + 1))
    sleep 0.5
done
[ -n "$pid" ] || {
    echo "noctalia-shell did not come up." >&2
    exit 1
}

# --- 3. present the lock surface again (adopts the lock now restore is on) ---
# Target by pid: `qs -c noctalia-shell ipc` can fail to resolve the system
# /etc/xdg instance ("No running instances"), but --pid always hits the right one.
qs ipc --pid "$pid" call lockScreen lock || true

echo "Lock surface re-presented. Switch to the graphical VT (Ctrl+Alt+F1) and enter your password."
