--!nonstrict
-- Rename the focused display's active workspace: one input, Enter commits
-- through `bsctl ws name set` (an empty name resets to the number), Escape
-- closes. Opened by HYPER+R and by right-clicking the pills. Reordering is a
-- drag in the bar itself, so this panel has exactly one job.
local lib = require("./lib.luau")

local workspaces = noctalia.state.get("workspaces") or {}
local displays = noctalia.state.get("displays") or {}
local target = nil -- { ws, bs, name } snapshot taken at open
local openNonce = 0 -- a fresh input key per open, so it takes focus each time

local function activeWorkspace()
  local ws = nil
  for _, d in ipairs(displays) do
    if d.focused == true and d.activeWs ~= nil then ws = d.activeWs end
  end
  if ws == nil then return nil end
  for _, w in ipairs(workspaces) do
    if w.ws == ws then
      local name = w.name or ""
      if name == tostring(w.ws) or name == tostring(w.bs) then name = "" end
      return { ws = w.ws, bs = w.bs, name = name }
    end
  end
  return { ws = ws, bs = nil, name = "" }
end

function render()
  if target == nil then
    panel.render(ui.column({ gap = 8, align = "stretch" }, {
      ui.label({ text = "No active workspace to rename.", color = "on_surface_variant" }),
    }))
    return
  end
  local ws = target.ws
  panel.render(ui.column({ gap = 10, align = "stretch" }, {
    ui.row({ align = "center", justify = "space_between" }, {
      ui.label({ text = `Rename workspace {target.bs or ws}`, fontSize = 15, fontWeight = "bold" }),
      ui.button({ glyph = "x", variant = "ghost", controlSize = "sm", onClick = function() panel.close() end }),
    }),
    ui.input({
      key = `name-{ws}-{openNonce}`, value = target.name, focus = true, controlSize = "md",
      placeholder = "Name (empty resets to the number) — Enter to apply",
      onSubmit = function(v)
        lib.bsctl({ "ws", "name", "set", "--ws-id", ws, "--name", v })
        panel.close()
      end,
    }),
  }))
end

noctalia.state.watch("workspaces", function(v) workspaces = v or {} end)
noctalia.state.watch("displays", function(v) displays = v or {} end)

function onOpen(_ctx)
  openNonce += 1
  target = activeWorkspace()
  render()
end
