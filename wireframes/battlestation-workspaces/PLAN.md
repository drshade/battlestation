# The Deck — Queue-view redesign plan

Implementation plan for reworking the **Queue view** of `Panel.qml`
(`stow/home/noctalia/.config/noctalia/plugins/battlestation-workspaces/Panel.qml`)
toward the wireframes in this directory. Board view is **deferred** (not yet
well thought through). This tracks the Queue only.

The redesign is mostly a **restyle plus a few additions** on top of plumbing
that already works — no new `asks` contract surface for the Queue.

## Status: SHIPPED

v2 is now **the** Deck, bound to **`SUPER+A`** (`panelMode = "asks"`). The old v1
view, its proxy+drop-line drag machinery, and the `asks2`/`SUPER+D` plumbing
have been removed. Internal identifiers (`asks2Col`, `v2*`, `card2`) are kept as
historical names to avoid a churny rename. The section below documents how it
got here.

## Approach: v2 as a second skin, not a fork

Ship v2 **alongside** the current Deck so we can A/B live and roll back freely:

- Current Deck stays on **`SUPER+A`** (`panelMode = "asks"`) — untouched.
- New Deck opens on **`SUPER+D`** (`panelMode = "asks2"`) — verified free.

Crucially, v2 is a **new view/skin inside the existing `Panel.qml`, sharing
100% of the asks logic on `root`** (identity-stable rows, drag reorder, draft
autosave, delivery fade, geometry snapshot, Process wiring). It is NOT a
separate panel file — a fork would duplicate the SmartPanel geometry contract
and all that logic and immediately drift. The v2 block only differs in layout;
it calls the same `root` functions the v1 block does. When v2 wins, delete the
v1 view block — there is no logic to migrate.

Wiring:
- `keybinds.lua`: `SUPER+D` → IPC `asksV2`.
- `Main.qml`: `toggleAsksPanel(screen, buttonItem, mode)` (default `"asks"`);
  IPC `asksV2()` stages `"asks2"`.
- `Panel.qml`: an `asksMode` helper (`mode === "asks" || mode === "asks2"`)
  gates the shared width/height/Timer/onCompleted; a new `visible: mode ===
  "asks2"` view renders the skin.

## Settled decisions

- **Inject / Retrigger — unchanged.** Keep the current behavior exactly.
  Inject = the per-turn `UserPromptSubmit` nudge (`asks inject`). Retrigger =
  write into the session's kitty terminal when `waiting`, else queue and flush
  on the end-turn hook. The wireframe's tooltip wording is just doc copy to
  correct; the toggles keep their meaning and persisted `pluginSettings` keys
  (`asksInject`, `asksRetrigger`).
- **Blocking → a status flag, not the loud channel.** Render it as the small
  `▹ blocking` tag (wireframe). **Urgency owns the left accent bar.** (Departs
  from today's `dotColor()`, where the dot is the live blocking signal — the
  designer's call, and blocking is used rarely enough that a status is fine.)
- **Theme = dynamic Noctalia.** The amber palette in the wireframes is only the
  base theme the designer mocked against. All colors map to Noctalia scheme
  tokens (`Color.mPrimary`, `mError`, `mTertiary`, `mOnSurface`, …) so the Deck
  stays cohesive with the active theme.
- **Expand/collapse is required.** The whole-panel-redraw jank is already
  solved (identity-stable rows via `askIds`/`askById`; snapshotted
  `contentPreferredHeight`). Expansion happens *inside* the fixed frame.

## Net-new work

- **Harness identity icons — reuse the existing system as-is, no redesign.**
  Render each card's harness via the current `BotIcon.qml` + `Cfg.qml` kind
  registry against the real `assets/{claude,codex,agy}/` SVG set (per-status
  `thinking`/`tool`/`waiting` files plus emotes). Status comes from the same
  `main.statusBySid` (kind from `kindBySid`) the bar pills already use. The
  wireframe's inlined single-path glyphs were only stand-ins — we do NOT
  recreate glyphs or tint a flat path; status is already encoded by the icon
  set. The `waiting` state is the "Trigger will actually fire" signal — same
  fact as `willWake()`, so the card's Trigger/Enqueue label should read
  consistent with the icon.
- **Project / workspace filter chips.** A chip row to narrow the global queue
  to one workspace (new — the current panel has no per-ws filter). "All
  projects" default.
- **Terminal-styled expanded body.** Monospace transcript with a prompt header
  and blinking cursor for the ask body, replacing today's plain wrapped text.
- **Chip metadata.** Project + agent + id + age as typed chips; typed glyph
  (`?` / `⇄` / `ℹ`) beside the accent bar.

## Kept, mapped to existing `asks` fields

- Drag-handle reorder → `asks order set` (identity-stable, floating proxy + drop line).
- Delivery fade: `answered → awaiting pickup → delivered ✓`, plus the default-on
  **Hide delivered** toggle (the auto-fade). Keep both.
- Auto-save draft (debounce / blur / collapse / close) → `asks reply`.
- One-click options, `working on it` / `later` tags → `asks note`.
- **Live Trigger/Enqueue button label** via `willWake()` (blocking → Enqueue;
  `waiting` idle asker → Trigger). The terminal-write gate is `status ==
  "waiting"` in `agents.rs:send_text`.
- Jump (`ws focus --session`), dismiss (`asks dismiss`), notify FYIs expand to
  body + "Got it".

## Spots that need care

- **Harness-icon live status in a card** — reuse `BotIcon`, bound to
  `statusBySid`/`kindBySid`, so it tracks status live without churning row
  delegates (content-only changes update in place; only id-sequence changes
  rebuild — anti-jank rule 1). Reusing `BotIcon` as-is is the point; the card
  just hosts it.
- **Terminal-body expansion inside the fixed frame** — the taller monospace
  transcript must scroll within the snapshotted panel geometry, not re-animate
  the SmartPanel frame (anti-jank rule 2).

## Status

**First pass landed (unverified at runtime).** Changes:
- `keybinds.lua`: `SUPER+D` → IPC `asksV2`.
- `Main.qml`: `toggleAsksPanel(…, mode)` + IPC `asksV2()` staging `"asks2"`.
- `Panel.qml`: `asksMode` gate on the shared machinery; `filterWs` (view
  filter folded into `syncAsks` membership); `openAsksCount` /
  `blockingAsksCount` / `projectChips` helpers; a `deckCfg` `Cfg` for the row
  BotIcons; and the full `asks2` view (header + toggles, project filter chips,
  reskinned cards with urgency accent bar + type glyph + chip metadata + live
  `BotIcon` + `▹ blocking` tag, terminal-styled expanded body, one-click
  options, reply→Trigger/Enqueue reusing the exact v1 draft wiring, note tags).

Linted with `qmllint` (clean bar unresolved `qs.*` imports). **Not yet
runtime-verified** — needs a Quickshell + Hyprland reload.

### Known gaps
- Monospace uses `Noto Sans Mono` literal; revisit against a Style token.
- Card width reuses v1's 880 (not yet "half-screen").

### Done since first pass
- Visual precision pass: ported wireframe px → Style tokens; terminal
  header-bar + divider; custom squared buttons; flush 3px urgency accent.
- Squared corners (`radiusXS`) across all v2 controls + the bar's workspace
  pills (`WorkspacePill.qml`).
- **Drag-reorder wired** — one shared drag machinery serves both skins via
  `dragRowsCol` (points at the active view's column); v2 grip added.
- **Filtered reorder** — dragging inside a project filter anchors the move onto
  the global order (`visibleOpenIds` + neighbour splice in `dragRelease`);
  hidden items keep their positions. Reduces to the plain splice when
  unfiltered.
- **Animated reorder (bar-style)** — v2's list is now a `ListView` +
  `DelegateModel` (`import QtQml.Models`). The grip drag calls
  `v2vm.items.move` live, so neighbours slide (`moveDisplaced`) and the dragged
  card settles (`move`), matching the bar. Commit persists via the same
  neighbour-anchor rule (`v2Commit`); the DelegateModel is left in the moved
  order so the stream echo reconciles without a flash. v1 keeps its
  proxy+drop-line drag untouched.
  - Not done: the dragged card does not yet *float under the cursor* pixel-by-
    pixel (the bar's `Drag.target`/`ParentChange` flourish) — it hops
    slot-to-slot with animation + a highlight/scale. Add if the hop feels off.

## Open

- How to verify: reload shell + `hyprctl reload`, then compare `SUPER+A`
  (v1) vs `SUPER+D` (v2).

## Reference

- Wireframes: `Battlestation Deck Wireframes.html` (standalone), `DeckPanel.dc.html`,
  `DeckCard.dc.html`, `Battlestation Deck Redesign.dc.html`; see `README.md`.
- Live code: `Panel.qml` (asks mode); contract in `ctl/src/lib.rs`,
  `ctl/src/asks.rs`, `ctl/src/agents.rs` (`send_text` / `wake`).
