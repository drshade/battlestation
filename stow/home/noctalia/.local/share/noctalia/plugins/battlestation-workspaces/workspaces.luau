--!nonstrict
-- Workspaces: rename and reorder. Rows are the GLOBAL battlespace order
-- across every display (bs N = row N); dragging a grip and dropping commits
-- the full order through `bsctl ws map set` (bsctl is the map's only
-- author; the stream then relabels every bar). The pencil (or opening with
-- context "rename" — the HYPER+R keybind) turns a row's name into an input;
-- Enter commits through `bsctl ws name set`, an empty name resets to the number.
local lib = require("./lib.luau")

local workspaces = noctalia.state.get("workspaces") or {}
local displays = noctalia.state.get("displays") or {}
local renaming = nil -- ws id in edit, or nil
local renameKey = 0 -- bumps so a fresh input grabs focus each time

local DRAG = "bs-workspace"

local function focusedActiveWs()
  for _, d in ipairs(displays) do
    if d.focused == true and d.activeWs ~= nil then return d.activeWs end
  end
  return nil
end

function onWsDropped(payload, value)
  local id = tonumber(payload)
  local to = tonumber(value)
  local ids = {}
  for _, w in ipairs(workspaces) do table.insert(ids, w.ws) end
  local from = lib.indexOf(ids, id)
  if not id or not to or not from then return end
  table.remove(ids, from)
  if from < to then to -= 1 end
  to = math.max(1, math.min(to, #ids + 1))
  table.insert(ids, to, id)
  local args = { "ws", "map", "set" }
  for _, x in ipairs(ids) do table.insert(args, x) end
  lib.bsctl(args)
end

local function commitName(ws, name)
  lib.bsctl({ "ws", "name", "set", "--ws-id", ws, "--name", name })
  renaming = nil
  render()
end

local function zone(i)
  return ui.dropZone({ key = `gap-{i}`, accepts = { DRAG }, value = tostring(i), onDrop = "onWsDropped",
    height = 3, radius = 4, expandOnDrag = true, hitSlop = 22, fill = "primary/0.12" })
end

local function row(w)
  local ws = w.ws
  local isActive = focusedActiveWs() == ws
  local name = w.name or ""
  if name == tostring(ws) or name == tostring(w.bs) then name = "" end
  local nameNode
  if renaming == ws then
    nameNode = ui.input({ key = `name-{ws}-{renameKey}`, value = name, focus = true, flexGrow = 1, controlSize = "sm",
      placeholder = "Workspace name (empty resets to the number)",
      onSubmit = function(v) commitName(ws, v) end })
  else
    nameNode = ui.label({ text = if name ~= "" then name else "(unnamed)", flexGrow = 1, maxLines = 1,
      color = if name ~= "" then "on_surface" else "on_surface_variant" })
  end
  return ui.row({ key = `ws-{ws}`, gap = 10, align = "center", padding = 6, radius = 10,
    fill = if isActive then "primary/0.18" else "surface_variant/0.3" }, {
    ui.dragSource({ key = `grip-{ws}`, dragType = DRAG, payload = tostring(ws), previewAncestor = 1, liftFromLayout = true,
      width = 20, height = 24, align = "center", justify = "center", tooltip = "Drag to reorder" }, {
      ui.glyph({ name = "grip-vertical", size = 14, color = "on_surface_variant" }),
    }),
    ui.label({ text = tostring(w.bs or "?"), fontWeight = "bold", width = 22, textAlign = "center" }),
    nameNode,
    ui.label({ text = `{w.display or "?"} · ws {ws} · {w.windows or 0}`, fontSize = 11, color = "on_surface_variant" }),
    ui.button({ glyph = if renaming == ws then "x" else "pencil", variant = "ghost", controlSize = "sm",
      tooltip = if renaming == ws then "Cancel" else "Rename",
      onClick = function()
        if renaming == ws then renaming = nil else renaming = ws; renameKey += 1 end
        render()
      end }),
    ui.button({ glyph = "arrow-right", variant = "ghost", controlSize = "sm", tooltip = "Focus",
      onClick = function() lib.bsctl({ "ws", "focus", "--ws-id", ws }); panel.close() end }),
  })
end

function render()
  local list = {}
  for i, w in ipairs(workspaces) do
    table.insert(list, zone(i))
    table.insert(list, row(w))
  end
  table.insert(list, zone(#workspaces + 1))
  panel.render(ui.column({ flexGrow = 1, gap = 10, align = "stretch" }, {
    ui.row({ align = "center", justify = "space_between" }, {
      ui.row({ gap = 10, align = "center" }, {
        ui.label({ text = "Workspaces", fontSize = 16, fontWeight = "bold" }),
        ui.label({ text = "battlespace order · drag to reorder, pencil to rename", fontSize = 12, color = "on_surface_variant" }),
      }),
      ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
    }),
    ui.scroll({ key = "rows", flexGrow = 1, gap = 4, align = "stretch" }, list),
  }))
end

noctalia.state.watch("workspaces", function(v) workspaces = v or {}; render() end)
noctalia.state.watch("displays", function(v) displays = v or {} end)

function onOpen(context)
  if context == "rename" then
    renaming = focusedActiveWs()
    renameKey += 1
  else
    renaming = nil
  end
  render()
end

function onClose()
  renaming = nil
end
