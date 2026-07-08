# The Deck — redesign wireframes

Interactive HTML wireframes for the Battlestation "Deck" widget (Quickshell / Noctalia
bar plugin). A design reference for reworking `Panel.qml` — not production code.

## What's here

| File | Purpose |
|------|---------|
| `Battlestation Deck Wireframes.html` | **Standalone, offline** — open directly in any browser. Self-contained (fonts + all components inlined). Start here. |
| `Battlestation Deck Redesign.dc.html` | Source: the canvas — intro, the Noctalia bar mock, and the annotated `1a`/`1b` option. |
| `DeckPanel.dc.html` | Source: the half-screen Deck panel (Queue + Board views, filters, header). |
| `DeckCard.dc.html` | Source: a single ask card (collapsed + terminal detail). |
| `support.js` | Tiny runtime for the `.dc.html` source files. |

> The `.dc.html` files are a lightweight component format. To open them live you need
> `support.js` alongside them and a static server. For a quick look, just open the
> standalone HTML above.

## The design in one screen

- **One panel, two interchangeable views** (toggle, top-right):
  - **Queue** — a single global priority list across all projects; blocking items sort to
    the top. A project-chip row narrows to one workspace.
  - **Board** — the same asks as `Inbox → Working → Blocked → Done` columns. `blocking`
    is an orthogonal `▹` flag, not a column. Respects the same project filter. Opening a
    card expands its column to full width so the detail has room.
- **Ask card** — typed glyph (`?` question / `⇄` review / `ℹ` notify), left accent bar for
  urgency, project + agent chips. Expands to a **terminal-style transcript** (monospace,
  preserved line breaks) with one-tap answer buttons, a stdin-style reply box, and
  `working on it` / `later` tags.
- **Agent harness icons** — the real Claude / Codex / Antigravity glyphs from
  `assets/`. Shape = harness identity; **fill colour = live status**:
  - `#3fb950` thinking · `#a371f7` tool use · `#d9895f` waiting · `#6f685c` idle
  - Icons pulse while busy. Multiple instances per workspace each render their own pip.
- **Global automation** in the header: **Inject** (send answers straight to the harness
  stdin) and **Auto-retrigger** (continue the harness terminal on item change vs. queue
  for later).

## Colour reference (dark Noctalia-style)

- Surface `#16130f` / card `#1e1a15` / terminal `#0c0a06`
- Amber primary `#d9a04a`, text `#f2ede2` / muted `#a89e8f`
- Urgency: high `#e0765f`, med `#d9a04a`, low `#9a8f7d`
- Status colours as listed above.

Fonts: **Geist** (UI) + **JetBrains Mono** (metadata, terminal).
