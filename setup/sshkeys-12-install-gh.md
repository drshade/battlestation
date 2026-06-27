# Installing the GitHub CLI (per machine)

**Goal:** install `gh`, used to authenticate and register this machine's SSH key
with GitHub. Optional — the web UI (sshkeys-13) works without it.

## Steps

```sh
sudo pacman -S github-cli
```

## Verify

```sh
gh --version
```
