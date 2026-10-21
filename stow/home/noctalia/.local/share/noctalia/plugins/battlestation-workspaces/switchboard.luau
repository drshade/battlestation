--!nonstrict
-- The Switchboard: the human-owned link table between agent sessions
-- (contract: ctl/src/lib.rs, "Agent communication"). Links are always
-- two-way and never transitive, and only the human creates them — so this
-- panel is the only place a link is born. Endpoints are addressed by their
-- public workspace + role name; session ids never cross this UI boundary.
--
-- A symmetric adjacency matrix: named live agents are both the rows and the
-- columns, and a cell toggle links/unlinks that pair (each pair appears
-- twice; both cells show the same state). Unnamed sessions are listed below
-- but cannot be wired — an agent must set_name first. Messages not yet
-- delivered to their recipient are listed under the matrix (the +N counts).
local lib = require("./lib.luau")

local comms = noctalia.state.get("comms") or {}
local pending = {} -- pairKey -> true while a link/unlink round-trips
local mail = {} -- undelivered messages, newest last (from `comms history`)
local mailUnread = -1 -- total unread the list was fetched for

local CELL = 120 -- column width (px): fits a two-word role name; HEAD is wider
local HEAD = 220

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

local function totalUnread()
  local n = 0
  for _, a in ipairs(comms.agents or {}) do n += tonumber(a.unread) or 0 end
  return n
end

-- The comms snapshot carries counts only; bodies and senders come from the
-- human-visible history. Refetch only when the unread total moves.
local function refreshMail()
  local want = totalUnread()
  if want == mailUnread then return end
  mailUnread = want
  if want == 0 then mail = {}; render(); return end
  lib.bsctl({ "comms", "history", "--format", "json", "--tail", "200" }, function(res)
    if res.exitCode ~= 0 then return end
    local list = noctalia.json.decode(res.stdout)
    if type(list) ~= "table" then return end
    local out = {}
    for _, m in ipairs(list) do
      if type(m.delivered_at) ~= "number" then table.insert(out, m) end
    end
    mail = out
    render()
  end)
end

local function age(created)
  local s = math.max(0, os.time() - math.floor(tonumber(created) or 0))
  if s < 60 then return `{s}s` end
  if s < 3600 then return `{s // 60}m` end
  if s < 86400 then return `{s // 3600}h` end
  return `{s // 86400}d`
end

local function who(e)
  return `{e and e.workspace or "?"} / {e and e.name or "?"}`
end

local function mailRow(m)
  local first = string.match(tostring(m.body or ""), "^[^\n]*") or ""
  return ui.row({ key = `mail-{m.id}`, gap = 8, align = "center", paddingH = 6 }, {
    ui.glyph({ name = "mail", size = 14, color = "error" }),
    ui.label({ text = `{who(m.from)}  →  {who(m.to)}`, fontSize = 12, fontWeight = "medium", maxLines = 1 }),
    ui.label({ text = age(m.created), fontSize = 11, color = "on_surface_variant", maxLines = 1 }),
    ui.label({ text = first, fontSize = 11, color = "on_surface_variant", maxLines = 1, flexGrow = 1 }),
  })
end

-- Row heading: status dot, kind mark, name, workspace, unread.
local function heading(a)
  return ui.row({ key = `head-{key(a)}`, width = HEAD, gap = 6, align = "center", paddingH = 6 }, {
    ui.box({ width = 7, height = 7, radius = 4, fill = statusColor[a.status] or "on_surface_variant" }),
    ui.image({ path = lib.statusIcon(a.kind, a.status), width = 14, height = 14, fit = "contain" }),
    ui.column({ flexGrow = 1, gap = 0 }, {
      ui.label({ text = a.name, fontWeight = "medium", maxLines = 1 }),
      ui.label({ text = a.workspace, fontSize = 11, color = "on_surface_variant", maxLines = 1 }),
    }),
    ui.label({ text = `+{a.unread}`, fontSize = 11, fontWeight = "bold", color = "error", visible = (tonumber(a.unread) or 0) > 0 }),
  })
end

-- Column heading: just the name, over its column.
local function colHeading(a)
  return ui.column({ key = `col-{key(a)}`, width = CELL, align = "center", gap = 0 }, {
    ui.label({ text = a.name, fontSize = 11, fontWeight = "medium", maxLines = 1, maxWidth = CELL - 8, textAlign = "center" }),
    ui.label({ text = a.workspace, fontSize = 10, color = "on_surface_variant", maxLines = 1, maxWidth = CELL - 8, textAlign = "center" }),
  })
end

local function cell(a, b, set)
  if a == b then
    -- The diagonal states the relationship once: a session is not its own peer.
    return ui.row({ key = `cell-{key(a)}-self`, width = CELL, height = 36, align = "center", justify = "center" }, {
      ui.glyph({ name = "minus", size = 12, color = "on_surface/0.25" }),
    })
  end
  local pk = pairKey(a, b)
  local isLinked = set[pk] == true
  return ui.row({ key = `cell-{key(a)}-{key(b)}`, width = CELL, height = 36, align = "center", justify = "center",
    fill = if isLinked then "primary/0.14" else nil, radius = 8 }, {
    ui.toggle({ checked = isLinked, enabled = pending[pk] == nil, onChange = function() toggle(a, b, isLinked) end }),
  })
end

function render()
  local agents = named()
  local set = linked()
  local unnamed = {}
  for _, a in ipairs(comms.agents or {}) do
    if a.name == nil or a.name == "" then table.insert(unnamed, a) end
  end
  local rows = {}
  if #agents < 2 then
    table.insert(rows, ui.label({ text = "Two named agents are needed before a link can be wired. An agent names itself with the Switchboard's set_name tool.", color = "on_surface_variant", maxLines = 3 }))
  else
    local cols = { ui.row({ width = HEAD, height = 8 }) }
    for _, a in ipairs(agents) do table.insert(cols, colHeading(a)) end
    table.insert(rows, ui.row({ key = "matrix-head", gap = 4, align = "end" }, cols))
    table.insert(rows, ui.separator({}))
    for _, a in ipairs(agents) do
      local cells = { heading(a) }
      for _, b in ipairs(agents) do table.insert(cells, cell(a, b, set)) end
      table.insert(rows, ui.row({ key = `row-{key(a)}`, gap = 4, align = "center", radius = 10, fill = "surface_variant/0.25" }, cells))
    end
  end
  if #mail > 0 then
    table.insert(rows, ui.separator({}))
    table.insert(rows, ui.label({ text = `Undelivered ({#mail}) — reaches the recipient when its turn ends or its next prompt starts`,
      fontSize = 12, fontWeight = "bold", maxLines = 1 }))
    for _, m in ipairs(mail) do table.insert(rows, mailRow(m)) end
  end
  local foot = {}
  if #unnamed > 0 then
    local parts = {}
    for _, a in ipairs(unnamed) do table.insert(parts, `{a.kind} · {a.workspace}`) end
    table.insert(foot, ui.label({ text = `Unnamed (not wirable until set_name): {table.concat(parts, ",  ")}`, fontSize = 11, color = "on_surface_variant", maxLines = 2 }))
  end
  panel.render(ui.column({ flexGrow = 1, gap = 10, align = "stretch" }, {
    ui.row({ align = "center", justify = "space_between" }, {
      ui.row({ gap = 10, align = "center" }, {
        ui.label({ text = "Switchboard", fontSize = 16, fontWeight = "bold" }),
        ui.label({ text = `{#agents} named · {#(comms.links or {})} links`, fontSize = 12, color = "on_surface_variant" }),
      }),
      ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
    }),
    ui.scroll({ key = "matrix", flexGrow = 1, gap = 4, align = "start" }, rows),
    ui.column({ gap = 2, align = "stretch" }, foot),
  }))
end

noctalia.state.watch("comms", function(v) comms = v or {}; refreshMail(); render() end)

function onOpen(_ctx)
  mailUnread = -1 -- refetch: delivery may have happened while closed
  refreshMail()
  render()
end
