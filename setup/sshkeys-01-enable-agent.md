# The SSH agent (per machine)

**Goal:** one ssh-agent available in every session, so a key is loaded once with
`ssh-add` and stays cached until logout.

There is no enable step: `stow/home/ssh-agent` owns the socket unit *and* its
`sockets.target.wants/` symlink, so stowing the repo is the enablement (see
AGENTS.md on repo-owned user services). On a fresh machine, after the first
stow, tell the running user manager about it:

```sh
systemctl --user daemon-reload
```

Log out and back in — the socket starts with the session and `environment.d`
exports `SSH_AUTH_SOCK`. To use it in an already-open session without re-login:

```sh
systemctl --user start ssh-agent.socket
set -gx SSH_AUTH_SOCK $XDG_RUNTIME_DIR/ssh-agent.socket   # fish
```
