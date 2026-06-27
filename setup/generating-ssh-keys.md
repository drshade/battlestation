# Generating an SSH key (per machine)

**Goal:** give this machine its own SSH key for GitHub / servers.

Keys are **per-machine**. Each machine generates its own keypair and registers
its *public* key; private keys are never copied between machines and never live
in this repo. This file is the procedure — the key material stays in `~/.ssh`.

## Steps

```sh
# 1. Generate an ed25519 key, commented with this machine's identity.
ssh-keygen -t ed25519 -C "tom@$(hostname)"
#    Accept the default path (~/.ssh/id_ed25519); set a passphrase.

# 2. Start the agent and load the key (so the passphrase is cached).
eval (ssh-agent -c)          # fish syntax; bash/zsh: eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519

# 3. Register the PUBLIC key with GitHub.
#    With the gh CLI:
gh auth login
gh ssh-key add ~/.ssh/id_ed25519.pub --title "$(hostname)"
#    Or copy it and paste at https://github.com/settings/keys :
cat ~/.ssh/id_ed25519.pub
```

## Verify

```sh
ssh -T git@github.com   # expect: "Hi <user>! You've successfully authenticated..."
```

## Notes

- The repo's `.gitignore` blocks private key material (`id_*`, `*.pem`, `*.key`)
  and allows only `*.pub`, so a private key cannot be committed by accident.
- `~/.ssh/config` (non-secret host aliases/settings) *can* be tracked later as a
  stow package (`stow/home/ssh/.ssh/config`) if it grows worth sharing. The keys
  themselves never are.
