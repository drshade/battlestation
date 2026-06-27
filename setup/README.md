# Setup notes

Reproducible, per-topic notes for setting up and customizing this machine
(CachyOS + Hyprland + Noctalia). Each file documents **what changed, why, and
the exact commands** so future-me (or Claude) can re-run it on a fresh install
or understand a past decision.

## Conventions

- One topic per file, named `<verb>-<subject>.md` (e.g. `removing-alacritty.md`).
- Keep commands copy-pasteable. Note anything that needs `sudo` or a manual step.
- These notes live **outside** the Stow packages, so they are never symlinked
  into `~/.config` — they document the dotfiles without cluttering them.
- This is a runbook, not a changelog. If a topic is later reversed, update the
  file (or delete it) rather than appending contradictions.

## Topics

The `.md` files in this directory are the topics — the listing is the index.
Filenames follow `<verb>-<subject>.md`, so the name *is* the summary
(`ls setup/`). There is deliberately no hand-maintained table here: it would
just duplicate the directory and drift out of sync.
