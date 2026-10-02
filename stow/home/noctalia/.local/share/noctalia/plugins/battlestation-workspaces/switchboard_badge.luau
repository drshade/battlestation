--!nonstrict
-- Switchboard entry point. Unlike the Deck badge it stays present when
-- empty: opening an empty Switchboard is how the human creates the first
-- link. The count is links (standing topology); unread traffic promotes the
-- badge to the attention colour until the recipient collects it.
local comms = noctalia.state.get("comms") or {}

local function render()
  local named, unread = 0, 0
  for _, a in ipairs(comms.agents or {}) do
    if a.name ~= nil and a.name ~= "" then named += 1 end
    unread += tonumber(a.unread) or 0
  end
  local links = #(comms.links or {})
  local color = if unread > 0 then "error" else "on_surface_variant"
  barWidget.render(ui.row({ gap = 4, align = "center" }, {
    ui.glyph({ name = "plug-connected", size = 14, color = color }),
    ui.label({ text = tostring(links), fontSize = 12, color = color }),
    ui.label({ text = `+{unread}`, fontSize = 11, fontWeight = "bold", color = "error", visible = unread > 0 }),
  }))
  local agentsTxt = if named == 1 then "1 named agent" else `{named} named agents`
  local linksTxt = if links == 1 then "1 link" else `{links} links`
  local unreadTxt = if unread == 0 then "no unread messages" else `{unread} unread`
  barWidget.setTooltip(`Switchboard · {agentsTxt} · {linksTxt} · {unreadTxt}`)
end

noctalia.state.watch("comms", function(v) comms = v or {}; render() end)

function update()
  noctalia.setUpdateInterval(30000)
  render()
end

function onClick()
  noctalia.togglePanel("battlestation/workspaces:switchboard")
end
