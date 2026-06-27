# SSH key + GitHub access (per machine)

Each machine makes its own key and registers the *public* half with GitHub.
Private keys never leave `~/.ssh` and never enter this repo.

## 1. Enable the ssh-agent

Socket-activated OpenSSH systemd unit; `SSH_AUTH_SOCK` is exported by the stowed
`ssh-agent` package.

```sh
systemctl --user enable --now ssh-agent.socket
```

Effective at next login. For the current session:

```sh
set -gx SSH_AUTH_SOCK $XDG_RUNTIME_DIR/ssh-agent.socket   # fish
```

## 2. Generate and load a key

```sh
ssh-keygen -t ed25519 -C "tom@$(hostname)"   # default path; set a passphrase
ssh-add ~/.ssh/id_ed25519                    # once per session, cached until logout
```

## 3. Register the public key with GitHub

Adds it to your GitHub *account*, not this repo. With `gh` (`sudo pacman -S
github-cli`):

```sh
gh auth login
gh ssh-key add ~/.ssh/id_ed25519.pub --title "$(hostname)"
```

Or paste `cat ~/.ssh/id_ed25519.pub` at <https://github.com/settings/keys>.

## Verify

```sh
ssh -T git@github.com   # "Hi <user>! You've successfully authenticated..."
```
