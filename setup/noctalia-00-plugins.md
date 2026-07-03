# Noctalia plugins

Plugin code under `~/.config/noctalia/plugins/` is **not tracked** (default-deny
in `.gitignore`): Noctalia's plugin manager installs and updates plugins, and
the tracked `plugins.json` records the source repo + enabled set — tracking the
code too would duplicate upstream and churn on every update.

**Fresh machine:** after stowing, open Noctalia's plugin manager and install
the plugins enabled in `plugins.json`.

Tracked exceptions (their source of truth is this repo):

- `battlestation-workspaces` — homegrown.
- `keybind-cheatsheet` — **temporary**: the tracked copy is exactly the union of
  two patches pending upstream in `noctalia-dev/legacy-v4-plugins`
  ([#937](https://github.com/noctalia-dev/legacy-v4-plugins/pull/937) apply
  "merge sequential" to hyprctl-sourced binds,
  [#938](https://github.com/noctalia-dev/legacy-v4-plugins/pull/938) Mod2/3/5 as
  first-class keys; branches live in the `~/dev/legacy-v4-plugins` fork).
  **De-vendor once both merge:** drop its `!` rules from `.gitignore`,
  `git rm -r --cached` the directory, update via the plugin manager, delete
  this exception from this note.
