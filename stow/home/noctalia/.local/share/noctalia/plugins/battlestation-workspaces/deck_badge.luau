--!nonstrict
-- Open-asks badge: the count of asks awaiting the human, coloured by the
-- LIVE signal — tertiary (the scheme's green) when at least one agent is
-- parked on an ask right now, holding its turn open; error (red) when the
-- open items sit with nobody waiting live, a backlog going stale. Collapses
-- while the queue is empty. Click toggles the Deck panel.
local asks = noctalia.state.get("asks") or {}

local function render()
  local open, blocking, est = 0, false, 0
  for _, r in ipairs(asks) do
    if r.state == "open" then
      open += 1
      if r.blocking == true then blocking = true end
      est += tonumber(r.estimate_min) or 0
    end
  end
  if open == 0 then
    barWidget.setVisible(false)
    return
  end
  barWidget.setVisible(true)
  local color = if blocking then "tertiary" else "error"
  barWidget.render(ui.row({ gap = 4, align = "center", fill = color .. "/0.22", radius = 9, paddingH = 7, minHeight = 22 }, {
    ui.glyph({ name = "inbox", size = 13, color = color }),
    ui.label({ text = tostring(open), fontSize = 12, fontWeight = "bold", color = color }),
  }))
  local what = if open == 1 then "1 open ask" else `{open} open asks`
  local live = if blocking then "an agent is waiting live" else "nobody waiting live"
  barWidget.setTooltip(`{what} · {live}` .. (if est > 0 then ` · ~{est}m of you` else ""))
end

noctalia.state.watch("asks", function(v) asks = v or {}; render() end)

function update()
  noctalia.setUpdateInterval(30000)
  render()
end

function onClick()
  noctalia.togglePanel("battlestation/workspaces:deck")
end
