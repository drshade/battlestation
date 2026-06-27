# Enabling the SSH agent (per machine)

**Goal:** one ssh-agent available in every session, so a key is loaded once with
`ssh-add` and stays cached until logout.

```sh
systemctl --user enable --now ssh-agent.socket
```

Log out and back in so `environment.d` exports `SSH_AUTH_SOCK`. To use it in an
already-open session without re-login:

```sh
set -gx SSH_AUTH_SOCK $XDG_RUNTIME_DIR/ssh-agent.socket   # fish
```
