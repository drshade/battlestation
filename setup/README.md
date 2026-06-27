# Setup notes

Per-topic runbooks for the out-of-band steps stowing the repo can't perform
(package installs/removals, enabling services). See `AGENTS.md` for what belongs
in a note.

## Naming

`<theme>-<order>-<description>.md`:

- `<theme>` — single hyphen-free token grouping related notes (`sshkeys`, `setup`).
- `<order>` — two-digit sequence within a theme; `00` for standalone notes.
- `<description>` — short, free-form.

e.g. `sshkeys-10-setup.md`, `setup-00-removing-alacritty.md`.

The sorted listing (`ls setup/`) is the index — grouped by theme, ordered within
each — so there is no hand-maintained table to drift.
