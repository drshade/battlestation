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

# 2. Load the key into the agent (see sshkeys-11-enable-agent.md for the agent).
ssh-add ~/.ssh/id_ed25519
```

Next: register the public key with GitHub — see sshkeys-13-register-key.md.

## Notes

- The repo's `.gitignore` blocks private key material (`id_*`, `*.pem`, `*.key`)
  and allows only `*.pub`, so a private key cannot be committed by accident.
- `~/.ssh/config` (non-secret host aliases/settings) *can* be tracked later as a
  stow package (`stow/home/ssh/.ssh/config`) if it grows worth sharing. The keys
  themselves never are.
