--!nonstrict
-- Claude plan usage: the brand mark + "session% · weekly%", resets in the
-- tooltip. Read from the world stream's `usage` section (bsctl caches the
-- provider call; this widget never polls anything itself).
local lib = require("./lib.luau")

local usage = noctalia.state.get("usage") or {}

local function resetText(iso, pattern)
  local t = lib.isoToEpoch(iso)
  if not t then return "?" end
  return noctalia.formatTime(pattern, t)
end

local function render()
  local c = usage.claude
  if not c then
    barWidget.setVisible(false)
    return
  end
  barWidget.setVisible(true)
  local s = tonumber(c.sessionPct) or -1
  local w = tonumber(c.weeklyPct) or -1
  local hot = s >= 80 or w >= 80
  barWidget.render(ui.row({ gap = 5, align = "center" }, {
    ui.image({ path = lib.kindIcon("claude"), width = 14, height = 14, fit = "contain" }),
    ui.label({ text = `{s}% / {w}%`, fontSize = 12, color = if hot then "error" else "on_surface" }),
  }))
  barWidget.setTooltip({
    { key = "Session", value = `{s}%  ·  resets {resetText(c.sessionResets, "%H:%M")}` },
    { key = "Weekly", value = `{w}%  ·  resets {resetText(c.weeklyResets, "%a %d %b")}` },
  })
end

noctalia.state.watch("usage", function(v) usage = v or {}; render() end)

function update()
  noctalia.setUpdateInterval(60000)
  render()
end
