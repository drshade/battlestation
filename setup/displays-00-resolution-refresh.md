# Optimising display resolution / refresh (per machine)

**Goal:** drive each monitor at the mode you actually want. The shared rule in
`hypr/config/monitors.lua` is `mode = "highrr"` — *highest refresh rate* — so on
a panel whose native mode isn't also its highest-refresh mode it picks the lower
resolution (e.g. an ultrawide offering 1080p@120 alongside 3440x1440@100 lands
on 1080p).

Machine-specific modes are **not** committed. Put them in an untracked
`hypr/config/monitors_local.lua` (gitignored; `monitors.lua` loads it via
`pcall(require, ...)`).

```sh
# 1. List a monitor's modes + description. Match by `desc:` (stable across
#    connectors/machines), not the connector name; read `availableModes`.
hyprctl monitors all
```

```lua
-- 2. ~/.config/hypr/config/monitors_local.lua — one hl.monitor per override.
hl.monitor({
    output   = "desc:HP Inc. OMEN 34c",
    mode     = "3440x1440@100",
    position = "auto",
    scale    = "auto",
})
```

```sh
# 3. Apply. A new file is picked up by `hyprctl reload`; editing an
#    already-loaded monitors_local.lua needs a full Hyprland restart (re-login).
hyprctl reload
```
