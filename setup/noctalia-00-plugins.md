# Noctalia plugins

Two kinds, by where the code lives:

- **Homegrown** (`battlestation/*`): source is this repo, under the `noctalia`
  package at `.local/share/noctalia/plugins/<name>/` — the shell's *local*
  plugin source. Stowing deploys them; `plugins.toml` enables them by id.
  Nothing else lives in that directory, so there is no ignore-list to keep.
- **Catalog** (`noctalia/*`, community ids): enabled by id in `plugins.toml`,
  fetched into `~/.local/state/noctalia/plugins/` by the shell on first start
  (needs network once). Never tracked — the id in `plugins.toml` is the whole
  registration.

**Fresh machine:** nothing beyond stowing. The first shell start clones the
catalogs and installs the enabled catalog plugins.

Writing one: `plugin.toml` manifest + Luau entry scripts; the API reference is
`noctalia.d.luau` in the official-plugins repo. Scripts hot-reload on save;
manifest changes need `noctalia msg plugins disable <id>` then `enable <id>`.
`noctalia plugins lint <dir>` checks a manifest against its scripts, and
`noctalia msg plugins list` shows load state. The shell log
(`~/.cache/noctalia/noctalia.log`) carries `[luau]` errors with the entry name.

Settings: typed `[[setting]]` entries in the manifest become the plugin's
settings UI; the stowed values live in `plugins.toml` under
`[plugin_settings."<id>"]`. GUI edits go to `~/.local/state/noctalia/settings.toml`
instead and shadow the stowed ones (see AGENTS.md gotchas).
