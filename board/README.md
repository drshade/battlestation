# board/ — the Battlestation menu-bar app (macOS)

The macOS surface for bsctl's world: a menu-bar status item (one dot per
live agent, the open-asks count) and a window with three tabs:

- **Board** — one lane per battlespace in bs order, a card per lead agent
  session with its subagent squad and open asks, and the Switchboard's links
  drawn between agents. Drag a lane title to reorder (`ws map set`),
  double-click to rename (`ws name set`), click an agent to jump to its
  window (`ws focus --session`), "Link agents" to wire two named agents
  (`comms link|unlink`).
- **Deck** — the asks queue, the counterpart of the Linux panel
  (`Panel.qml`, asks mode): reorder, reply drafts, answer / Trigger /
  Enqueue, note tags, dismiss, reopen, the Deck switch.
- **Links** — the Switchboard as a matrix.

It holds no state: it follows `bsctl status --format json --stream` and every
gesture is a bsctl verb, so it can never disagree with the bar on Linux.
`battlestation://board|deck|links` opens a tab (the AeroSpace keybinds use it).

`make build` compiles it (SwiftPM, Xcode command line tools), bundles it with
the harness glyphs from the bar plugin's `assets/` (their one source of
truth) and installs `~/Applications/Battlestation.app`; the stowed launchd
agent in `stow/home-darwin/board` starts it at login.

Layout check without a display: render any tab from a captured world —

```sh
bsctl status --format json > /tmp/world.json
BATTLESTATION_ASSETS=<plugin>/assets swift run --package-path board \
  Battlestation --snapshot /tmp/world.json /tmp/board.png board [dark]
```

(`Fixtures/world.json` is a sample world exercising every feature.)
