# Registering the SSH key with GitHub (per machine)

**Goal:** tell GitHub to trust this machine's *public* key so you can push over
SSH. Registers with your GitHub **account**, not this repo — no key material is
committed.

Prerequisite: a key from sshkeys-10-generate-key.md.

## Steps

With the gh CLI (see sshkeys-12-install-gh.md):

```sh
gh auth login
gh ssh-key add ~/.ssh/id_ed25519.pub --title "$(hostname)"
```

Or without gh — copy the public key and paste it at
<https://github.com/settings/keys>:

```sh
cat ~/.ssh/id_ed25519.pub
```

## Verify

```sh
ssh -T git@github.com   # expect: "Hi <user>! You've successfully authenticated..."
```
