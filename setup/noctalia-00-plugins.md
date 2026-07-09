# Noctalia plugins

Plugin code under `~/.config/noctalia/plugins/` is **not tracked** (default-deny
in `.gitignore`): Noctalia's plugin manager installs and updates plugins, and
the tracked `plugins.json` records the source repo + enabled set — tracking the
code too would duplicate upstream and churn on every update.

**Fresh machine:** after stowing, open Noctalia's plugin manager and install
the plugins enabled in `plugins.json`.

Tracked exceptions (their source of truth is this repo):

- `battlestation-workspaces` — homegrown.
- `dictation` — homegrown; see `setup/dictation-00-setup.md` for its
  out-of-band deps (whisper model, packages).
- `kde-connect` — **temporary**: upstream's 5s refresh loop calls the
  daemon's `forceOnNetworkChange` DBus method, which tears down every live
  device link — the widget caused the very connection-flapping it displayed
  (diagnosed live 2026-07-05; the tracked copy polls `getDevices` instead).
  Fix submitted upstream: WerWolv/noctalia-kde-connect#21. **De-vendor once
  it merges AND the noctalia registry mirror ships it** (the registry
  lagged master before — don't assume the merge alone updates the installed
  copy): drop its `!` rules from `.gitignore`, `git rm -r --cached` the
  directory, update via the plugin manager, verify the installed copy polls
  `getDevices`, delete this exception from this note.
