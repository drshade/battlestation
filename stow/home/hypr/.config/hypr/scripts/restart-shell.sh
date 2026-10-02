#!/usr/bin/env sh
# Restart the Noctalia shell (HYPER+Backspace). The shell is single-instance
# and holds a lock while it tears down; a relaunch racing that teardown fails
# with "already running", so wait for the old process to be gone first (and
# finish it off if it lingers — a stuck shutdown must not strand the desktop
# without a bar).
set -eu
pkill -x noctalia || true
i=0
while pgrep -x noctalia >/dev/null && [ "$i" -lt 30 ]; do
    i=$((i + 1))
    sleep 0.2
done
if pgrep -x noctalia >/dev/null; then
    pkill -9 -x noctalia || true
    sleep 0.5
fi
exec noctalia
