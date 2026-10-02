--!nonstrict
-- Microphone picker. Sources come from `pw-dump` (direct argv), filtered to
-- media.class == "Audio/Source". The choice is stored by node.name — stable
-- across reboots, unlike node ids — in the plugin's data dir; "" follows the
-- system default.
local sources = {} -- { {name, desc} }
local current = ""
local status = ""

local function dataFile()
  local dir = noctalia.pluginDataDir()
  return if dir then dir .. "/mic.json" else nil
end

local function load()
  local f = dataFile()
  current = ""
  if f then
    local txt = noctalia.readFile(f)
    local d = if txt then noctalia.json.decode(txt) else nil
    current = (d and d.node) or ""
  end
end

local function save(node)
  local f = dataFile()
  if not f then
    status = "No data dir available."
    return render()
  end
  local body = noctalia.json.encode({ node = node })
  local ok, err = noctalia.writeFile(f, body)
  if not ok then
    status = `Could not save: {err}`
  else
    current = node
    status = if node == "" then "Following the system default source." else `Using {node}.`
  end
  render()
end

local function refresh()
  status = "Scanning PipeWire sources…"
  render()
  noctalia.runAsync({ "pw-dump" }, function(res)
    if res.exitCode ~= 0 then
      status = `pw-dump failed (exit {res.exitCode}).`
      return render()
    end
    local arr = noctalia.json.decode(res.stdout or "")
    sources = {}
    for _, item in ipairs(arr or {}) do
      local props = item.info and item.info.props
      if props and props["media.class"] == "Audio/Source" then
        table.insert(sources, { name = props["node.name"] or "", desc = props["node.description"] or props["node.nick"] or props["node.name"] or "?" })
      end
    end
    status = `{#sources} source(s).`
    render()
  end, 10000)
end

local function row(name, desc)
  local picked = current == name
  return ui.row({ key = `src-{name}`, gap = 10, align = "center", padding = 8, radius = 10,
    fill = if picked then "primary/0.18" else "surface_variant/0.3" }, {
    ui.glyph({ name = if picked then "circle-check" else "circle", size = 14, color = if picked then "primary" else "on_surface_variant" }),
    ui.column({ flexGrow = 1, gap = 2 }, {
      ui.label({ text = desc, fontWeight = "medium", maxLines = 1 }),
      ui.label({ text = if name == "" then "default source" else name, fontSize = 11, color = "on_surface_variant", maxLines = 1 }),
    }),
    ui.button({ text = if picked then "Selected" else "Use", variant = if picked then "primary" else "outline", controlSize = "sm", enabled = not picked,
      onClick = function() save(name) end }),
  })
end

function render()
  local list = { row("", "System default") }
  for _, s in ipairs(sources) do table.insert(list, row(s.name, s.desc)) end
  panel.render(ui.column({ flexGrow = 1, gap = 10, align = "stretch" }, {
    ui.row({ align = "center", justify = "space_between" }, {
      ui.label({ text = "Dictation microphone", fontSize = 16, fontWeight = "bold" }),
      ui.row({ gap = 6 }, {
        ui.button({ glyph = "refresh", variant = "ghost", tooltip = "Rescan", onClick = function() refresh() end }),
        ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
      }),
    }),
    ui.scroll({ key = "rows", flexGrow = 1, gap = 4, align = "stretch" }, list),
    ui.label({ text = status, fontSize = 11, color = "on_surface_variant" }),
  }))
end

function onOpen(_ctx)
  load()
  refresh()
end
