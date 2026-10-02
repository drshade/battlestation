--!nonstrict
-- The Switchboard: the human-owned link table between agent sessions
-- (contract: ctl/src/lib.rs, "Agent communication"). Links are always
-- two-way and never transitive, and only the human creates them — so this
-- panel is the only place a link is born. Endpoints are addressed by their
-- public workspace + role name; session ids never cross this UI boundary.
-- Unnamed sessions are listed but cannot be wired: an agent must set_name first.
local lib = require("./lib.luau")

local comms = noctalia.state.get("comms") or {}
local pending = {} -- pairKey -> true while a link/unlink round-trips

local function key(a)
  return `{a.workspace}/{a.name}`
end
local function pairKey(a, b)
  local ka, kb = key(a), key(b)
  return if ka < kb then ka .. "|" .. kb else kb .. "|" .. ka
end
local function named()
  local out = {}
  for _, a in ipairs(comms.agents or {}) do
    if a.name ~= nil and a.name ~= "" then table.insert(out, a) end
  end
  table.sort(out, function(x, y)
    if x.workspace ~= y.workspace then return x.workspace < y.workspace end
    return x.name < y.name
  end)
  return out
end
local function linked()
  local set = {}
  for _, l in ipairs(comms.links or {}) do
    set[pairKey(l.a, l.b)] = true
  end
  return set
end
local function toggle(a, b, isLinked)
  local pk = pairKey(a, b)
  pending[pk] = true
  lib.bsctl({ "comms", if isLinked then "unlink" else "link",
    "--a-workspace", a.workspace, "--a-name", a.name, "--b-workspace", b.workspace, "--b-name", b.name },
    function() pending[pk] = nil; render() end)
  render()
end

local statusColor = { thinking = "tertiary", tooling = "secondary", waiting = "on_surface_variant" }

local function agentLabel(a)
  return ui.row({ gap = 6, align = "center", flexGrow = 1 }, {
    ui.image({ path = lib.statusIcon(a.kind, a.status), width = 14, height = 14, fit = "contain" }),
    ui.label({ text = a.name, fontWeight = "medium", maxLines = 1 }),
    ui.label({ text = a.workspace, fontSize = 11, color = "on_surface_variant", maxLines = 1 }),
    ui.label({ text = `+{a.unread}`, fontSize = 11, fontWeight = "bold", color = "error", visible = (tonumber(a.unread) or 0) > 0 }),
  })
end

function render()
  local agents = named()
  local set = linked()
  local unnamed = #(comms.agents or {}) - #agents
  local rows = {}
  if #agents < 2 then
    table.insert(rows, ui.label({ text = "Two named agents are needed before a link can be wired. An agent names itself with the Switchboard's set_name tool.", color = "on_surface_variant", maxLines = 3 }))
  end
  for i = 1, #agents do
    for j = i + 1, #agents do
      local a, b = agents[i], agents[j]
      local pk = pairKey(a, b)
      local isLinked = set[pk] == true
      table.insert(rows, ui.row({ key = `pair-{pk}`, gap = 10, align = "center", padding = 8, radius = 10,
        fill = if isLinked then "primary/0.18" else "surface_variant/0.3" }, {
        agentLabel(a),
        ui.glyph({ name = if isLinked then "arrows-left-right" else "minus", size = 14, color = if isLinked then "primary" else "on_surface_variant" }),
        agentLabel(b),
        ui.toggle({ checked = isLinked, enabled = pending[pk] == nil, onChange = function() toggle(a, b, isLinked) end }),
      }))
    end
  end
  local live = {}
  for _, a in ipairs(agents) do
    table.insert(live, ui.row({ gap = 6, align = "center" }, {
      ui.box({ width = 7, height = 7, radius = 4, fill = statusColor[a.status] or "on_surface_variant" }),
      ui.label({ text = `{a.name} · {a.workspace}`, fontSize = 11, color = "on_surface_variant" }),
    }))
  end
  panel.render(ui.column({ flexGrow = 1, gap = 10, align = "stretch" }, {
    ui.row({ align = "center", justify = "space_between" }, {
      ui.row({ gap = 10, align = "center" }, {
        ui.label({ text = "Switchboard", fontSize = 16, fontWeight = "bold" }),
        ui.label({ text = `{#agents} named · {#(comms.links or {})} links` .. (if unnamed > 0 then ` · {unnamed} unnamed` else ""), fontSize = 12, color = "on_surface_variant" }),
      }),
      ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
    }),
    ui.scroll({ key = "pairs", flexGrow = 1, gap = 4, align = "stretch" }, rows),
    ui.separator({}),
    ui.column({ gap = 2 }, live),
  }))
end

noctalia.state.watch("comms", function(v) comms = v or {}; render() end)

function onOpen(_ctx)
  render()
end
