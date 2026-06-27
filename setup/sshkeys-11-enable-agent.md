# Enabling the SSH agent (per machine)

**Goal:** one ssh-agent available in every session, so a key is loaded once with
`ssh-add` and stays cached until logout.

This uses the socket-activated systemd user unit shipped with OpenSSH (the agent
starts on first use). `SSH_AUTH_SOCK` is exported by a tracked config file —
`stow/home/ssh-agent/.config/environment.d/ssh-agent.conf` — so it reproduces via
stow and is not duplicated here.

## Steps

```sh
systemctl --user enable --now ssh-agent.socket
```

Log out and back in so `environment.d` exports `SSH_AUTH_SOCK`. To use it in an
already-open session without re-login:

```sh
set -gx SSH_AUTH_SOCK $XDG_RUNTIME_DIR/ssh-agent.socket   # fish
```

## Per session

```sh
ssh-add        # load your key (prompts for passphrase); cached until logout
ssh-add -l     # list loaded keys
```

## Verify

```sh
echo $SSH_AUTH_SOCK   # -> /run/user/<uid>/ssh-agent.socket
ssh-add -l            # lists your key once added
```
