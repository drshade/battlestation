# Setup notes

Per-machine, out-of-band steps that stowing the repo can't do — package
install/removal, enabling services. If `stow` already does it, it's not a note.

## Naming — `<theme>-<order>-<description>.md`

- `<theme>` — one hyphen-free token grouping related notes. Either an ordered
  **procedure** read `00`→N — later steps build on earlier ones implicitly,
  shared context stated once, no cross-refs (e.g. `sshkeys`) — or a **collection**
  of independent items (e.g. `debloat`, one note per thing removed).
- `<order>` — two digits, `00`-based, `+1` within the theme.
- `<description>` — short, free-form.

e.g. `sshkeys-00-generate-key.md` … `sshkeys-03-register-key.md`,
`debloat-00-removing-alacritty.md`.

`ls setup/` is the index — no table to maintain.

## Style

Terse; assume a competent reader. Document the non-obvious command, not the
obvious — no spoon-feeding, no scaffolding, no filler.
