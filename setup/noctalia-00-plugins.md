# Noctalia plugins

Plugin code under `~/.config/noctalia/plugins/` is **not tracked** (default-deny
in `.gitignore`): Noctalia's plugin manager installs and updates plugins, and
the tracked `plugins.json` records the source repo + enabled set — tracking the
code too would duplicate upstream and churn on every update.

**Fresh machine:** after stowing, open Noctalia's plugin manager and install
the plugins enabled in `plugins.json`.

Tracked exceptions (their source of truth is this repo):

- `battlestation-workspaces` — homegrown.
