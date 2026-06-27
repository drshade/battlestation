# Registering the SSH key with GitHub (per machine)

**Goal:** tell GitHub to trust this machine's *public* key so you can push over
SSH. Registers with your GitHub **account**, not this repo — no key material is
committed.

```sh
gh auth login
gh ssh-key add ~/.ssh/id_ed25519.pub --title "$(hostname)"
```
