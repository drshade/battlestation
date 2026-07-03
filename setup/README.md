# Setup notes

Per-machine, out-of-band steps that stowing the repo can't do — package
install/removal, enabling services. If `stow` already does it, it's not a note.

## Naming — `<theme>-<order>-<description>.md`

- `<theme>` — one hyphen-free token grouping related notes. Either an ordered
  **procedure** read `00`→N — later steps build on earlier ones implicitly,
  shared context stated once, no cross-refs (e.g. `sshkeys`) — or a **collection**
  of independent items (e.g. `debloat`, one note per thing removed).
- `<order>` — two digits, ordering steps within a **procedure**; in a
  **collection** it's irrelevant, so just reuse `00` (notes sort by description).
- `<description>` — short, free-form.

e.g. `sshkeys-00-generate-key.md` … `sshkeys-03-register-key.md`,
`debloat-00-removing-alacritty.md`.

`ls setup/` is the index — no table to maintain.

## Style

Terse; assume a competent reader. Document the non-obvious command, not the
obvious — no spoon-feeding, no scaffolding, no filler.

**No history or origin stories.** A note serves someone mid-rebuild; how the
old state came to be, what replaced what, and who installed it are noise to
that reader (an early rust note carried a provenance tale — anti-pattern).
History lives in git commit messages and PLAN.md outcomes; a note states only
the current steps and, per AGENTS.md principle #5, the *why* of the current
choice when it isn't obvious.
