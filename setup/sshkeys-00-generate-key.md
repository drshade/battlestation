# Generating an SSH key (per machine)

**Goal:** give this machine its own SSH key for GitHub / servers.

```sh
# 1. Generate an ed25519 key, commented with this machine's identity.
ssh-keygen -t ed25519 -C "tom@$(hostname)"
```
