# Setup notes

Reproducible, per-topic notes for setting up and customizing this machine
(CachyOS + Hyprland + Noctalia). Each file documents **what changed, why, and
the exact commands** so future-me (or Claude) can re-run it on a fresh install
or understand a past decision.

## Conventions

- One topic per file, named **`<theme>-<order>-<description>.md`**:
  - `<theme>` — a single hyphen-free token grouping related notes (`sshkeys`,
    `setup`). Files sharing a theme sort together.
  - `<order>` — a two-digit number ordering steps *within* a theme
    (`10`, `11`, …). Use `00` for standalone notes with no sequence.
  - `<description>` — short, free-form (may contain hyphens).
  - e.g. `sshkeys-10-generate-key.md`, `setup-00-removing-alacritty.md`.
- Keep commands copy-pasteable. Note anything that needs `sudo` or a manual step.
- These notes live **outside** the Stow packages, so they are never symlinked
  into `~/.config` — they document the dotfiles without cluttering them.
- This is a runbook, not a changelog. If a topic is later reversed, update the
  file (or delete it) rather than appending contradictions.

## Topics

The `.md` files in this directory are the topics — the sorted listing (`ls
setup/`) *is* the index: grouped by theme, ordered within each. The naming
carries both grouping and order, so there is deliberately no hand-maintained
table here — it would just duplicate the directory and drift out of sync.
