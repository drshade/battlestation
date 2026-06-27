# Setup notes

Per-machine, out-of-band steps that stowing the repo can't do — package
install/removal, enabling services. If `stow` already does it, it's not a note.

## Naming — `<theme>-<order>-<description>.md`

- `<theme>` — one hyphen-free token. A theme is a **single procedure**: its files
  are read together, in order, like sections of one document. Later steps may
  build on earlier ones, left implicit — no cross-references, and shared context
  is stated once in the first step.
- `<order>` — two digits, `00`-based, `+1` per step within the theme.
- `<description>` — short, free-form.

e.g. `sshkeys-00-generate-key.md` … `sshkeys-03-register-key.md`,
`setup-00-removing-alacritty.md`.

`ls setup/` is the index — no table to maintain.

## Style

Terse; assume a competent reader. Document the non-obvious command, not the
obvious — no spoon-feeding, no scaffolding, no filler.
