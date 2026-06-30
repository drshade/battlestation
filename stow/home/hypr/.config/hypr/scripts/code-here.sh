#!/usr/bin/env bash
# code-here.sh — open VS Code at the focused terminal's working directory.
# Directory comes from focused-cwd.sh (shared resolver). When that's empty (the
# focused window isn't a terminal, e.g. a browser or VS Code itself), we open
# VS Code without a folder rather than dumping you into $HOME as a workspace.
set -euo pipefail

here=$(dirname "$(readlink -f "$0")")
dir=$("$here/focused-cwd.sh" || true)

if [[ -n $dir && -d $dir ]]; then
    # cd first so code-oss's process cwd becomes the project folder. That makes
    # VS Code a cwd "anchor": Hyper+T while it's focused lands in the same place
    # (focused-cwd.sh reads code-oss's own /proc/<pid>/cwd).
    cd "$dir"
    exec code-oss "$dir"
else
    exec code-oss
fi
