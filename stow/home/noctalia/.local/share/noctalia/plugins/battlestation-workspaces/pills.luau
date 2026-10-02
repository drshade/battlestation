--!nonstrict
-- Workspace pills for THIS bar's output: one capsule per workspace in
-- battlespace order, labelled by bs-id ("3 - name"), carrying one animated
-- bot per agent session on it (plus smaller sub-bots for running subagents).
-- Click a pill → `bsctl ws focus`. Right-click anywhere → the Workspaces
-- panel (rename / reorder). Pure presentation over the world state.
--
-- Each bar instance shows ONLY the workspaces on its own output, but the
-- order and bs numbers stay GLOBAL — bs-ids are what SUPER+N addresses, so a
-- pill keeps its number even when pills before it live on another display.
-- A workspace whose output is unknown fails OPEN (every bar shows it).
local lib = require("./lib.luau")

local workspaces = noctalia.state.get("workspaces") or {}
local displays = noctalia.state.get("displays") or {}
local agents = noctalia.state.get("agents") or {}

-- ---- settings ----------------------------------------------------------------
local cfg = {}
local function loadCfg()
  cfg.capitalize = noctalia.getConfig("capitalize_names") ~= false
  cfg.outline = noctalia.getConfig("outline_pills") == true
  cfg.hideTrailing = noctalia.getConfig("hide_trailing") ~= false
  cfg.focused = noctalia.getConfig("focused_color") or "primary"
  cfg.occupied = noctalia.getConfig("occupied_color") or "secondary"
  cfg.empty = noctalia.getConfig("empty_color") or "grey"
  cfg.activeMs = tonumber(noctalia.getConfig("active_ms")) or 1000
  cfg.waitS = tonumber(noctalia.getConfig("wait_s")) or 30
  cfg.jitter = tonumber(noctalia.getConfig("jitter")) or 0.35
end
loadCfg()

local D = 18 -- bot size (px)
local SUB = 0.6 -- sub-bot size vs its commander
local TICK = 100 -- emote engine resolution (ms)

-- ---- bots: stable identity per session -----------------------------------------
-- Keyed by SESSION ID so a bot keeps its status/title/kind across stream
-- lines; per workspace the order is first-seen (a status change never
-- reshuffles a live bot's slot).
local sidsByWs = {}
local bySid = {} -- sid -> agent record
local bots = {} -- sid -> emote state {nextAt, face, faceUntil, motion, motionAt}

local function rnd(lo, hi)
  return lo + math.random() * (hi - lo)
end
local function nextGap(status)
  local base = if status == "waiting" then cfg.waitS * 1000 else cfg.activeMs
  return base * rnd(1 - cfg.jitter, 1 + cfg.jitter)
end
local function emote(sid, now, force)
  local b = bots[sid]
  local a = bySid[sid]
  if not b or not a then return end
  if not force and (b.face or b.motion) then return end
  local pool = lib.emotePool(a.kind, a.status)
  local e = pool[math.random(1, #pool)]
  if e == "bounce" or e == "wiggle" then
    b.motion, b.motionAt, b.face = e, now, nil
  else
    -- Resolve the asset NOW: a face picked under one status must not be
    -- re-pathed if the status flips before it expires (tool-squint.svg
    -- does not exist; thinking-squint.svg does).
    b.face, b.facePath, b.faceUntil, b.motion = e, lib.faceIcon(a.kind, a.status, e), now + (lib.faceHold[e] or 700), nil
  end
end

local function applyAgents(list)
  agents = list or {}
  local seen = {}
  local fresh = {}
  local newBySid = {}
  for _, a in ipairs(agents) do
    if a.session then
      newBySid[a.session] = a
      local ws = tostring(a.ws)
      fresh[ws] = fresh[ws] or {}
      seen[ws] = seen[ws] or {}
      if not seen[ws][a.session] then
        seen[ws][a.session] = true
        table.insert(fresh[ws], a.session)
      end
    end
  end
  local out = {}
  for ws, list2 in pairs(fresh) do
    local kept = {}
    for _, sid in ipairs(sidsByWs[ws] or {}) do
      if seen[ws][sid] then table.insert(kept, sid) end
    end
    for _, sid in ipairs(list2) do
      if not lib.indexOf(kept, sid) then table.insert(kept, sid) end
    end
    out[ws] = kept
  end
  -- Re-pace a bot crossing the waiting<->active boundary; otherwise a bot
  -- that started out waiting would keep its slow timer after it got busy.
  local now = noctalia.nowMs()
  for sid, a in pairs(newBySid) do
    local b = bots[sid]
    local prev = bySid[sid]
    if not b then
      bots[sid] = { nextAt = now + nextGap(a.status) }
    elseif prev and (prev.status == "waiting") ~= (a.status == "waiting") then
      b.nextAt = now + nextGap(a.status)
    end
  end
  for sid in pairs(bots) do
    if not newBySid[sid] then bots[sid] = nil end
  end
  sidsByWs = out
  bySid = newBySid
end
applyAgents(agents)

-- ---- this bar's view of the compositor ------------------------------------------
local lastActive = nil
local function myDisplay()
  local mine = barWidget.outputName() or ""
  for _, d in ipairs(displays) do
    if d.name == mine then return d end
  end
  return nil
end

-- Visible = global order filtered to this output (unknown fails open), then
-- when hide_trailing, trimmed after the last occupied-or-active position.
local function visibleWorkspaces(disp)
  local mine = barWidget.outputName() or ""
  local activeWs = if disp and not disp.specialShowing then disp.activeWs else -1
  local vis = {}
  for _, w in ipairs(workspaces) do
    local out = w.display or ""
    if out == "" or mine == "" or out == mine then
      table.insert(vis, w)
    end
  end
  local mx = 1
  for i, w in ipairs(vis) do
    if (w.windows or 0) > 0 or w.ws == activeWs then mx = i end
  end
  if cfg.hideTrailing then
    local trimmed = {}
    for i = 1, math.min(mx, #vis) do trimmed[i] = vis[i] end
    vis = trimmed
  end
  return vis, activeWs
end

local function pillLabel(w)
  local name = w.name or ""
  local n = if (w.bs or 0) > 0 then tostring(w.bs) else "?"
  if name == "" or name == tostring(w.ws) or name == n then return n end
  name = string.sub(name, 1, 20)
  if cfg.capitalize then name = lib.capitalize(name) end
  return `{n} - {name}`
end

-- ---- render ----------------------------------------------------------------------
local lastSig = ""
local function botNode(sid, now, scale, commander)
  local a = bySid[sid]
  local b = bots[sid] or {}
  local size = math.floor(D * scale + 0.5)
  local path
  if b.face then
    path = b.facePath
  elseif commander then
    path = lib.restIcon(a.kind, a.status)
  else
    path = lib.statusIcon(a.kind, a.status)
  end
  -- Motions are kind-agnostic: bounce = a quick size hop, wiggle = a flicker
  -- between the status icon and the kind's neutral mark.
  if b.motion == "bounce" then
    local t = now - (b.motionAt or now)
    if t < 120 then size += 2 elseif t < 240 then size += 1 end
  elseif b.motion == "wiggle" then
    local t = now - (b.motionAt or now)
    if (t // 80) % 2 == 1 then path = lib.statusIcon(a.kind, "") end
  end
  local title = a.title
  if title == nil or title == "" then title = lib.kindLabel(a.kind) end
  -- Fixed-size slot around a variable-size image: an emote must never change
  -- the pill's width, or the whole bar centre shifts on every hop.
  local slot = math.floor(D * scale + 0.5) + 2
  local node = ui.row({ key = `slot-{sid}`, width = slot, height = slot, align = "center", justify = "center" }, {
    ui.image({ key = `bot-{sid}`, path = path, width = size, height = size, fit = "contain", tooltip = title }),
  })
  return node, path .. size
end

local function render(now)
  local disp = myDisplay()
  local vis, activeWs = visibleWorkspaces(disp)
  local focusedMon = disp == nil or disp.focused == true
  local sig = {}
  local pills = {}
  for _, w in ipairs(vis) do
    local role = if w.ws == activeWs then "focused" elseif (w.windows or 0) > 0 then "occupied" else "empty"
    local key = if role == "focused" then cfg.focused elseif role == "occupied" then cfg.occupied else cfg.empty
    local label = pillLabel(w)
    local children = {}
    local fg
    if cfg.outline then
      fg = if key == "grey" or key == "none" then "on_surface" else key
    else
      fg = lib.roleOn(key)
    end
    table.insert(children, ui.label({ text = label, color = fg, fontSize = 12, fontWeight = if role == "focused" then "bold" else "medium" }))
    for _, sid in ipairs(sidsByWs[tostring(w.ws)] or {}) do
      local a = bySid[sid]
      local subs = a.subagents or {}
      local node, s = botNode(sid, now, 1.0, #subs > 0)
      table.insert(children, node)
      table.insert(sig, s)
      for _, sub in ipairs(subs) do
        local title = (sub.type or "agent") .. (if sub.description and sub.description ~= "" then ` — {sub.description}` else "")
        table.insert(children, ui.image({ key = `sub-{sid}-{sub.id}`, path = lib.statusIcon(a.kind, a.status), width = math.floor(D * SUB + 0.5), height = math.floor(D * SUB + 0.5), fit = "contain", tooltip = title }))
      end
    end
    local wsId = w.ws
    local props = {
      key = `ws-{wsId}`, gap = 5, paddingH = 7, align = "center", radius = 9, minHeight = 22,
      opacity = if role == "focused" and not focusedMon then 0.7 else 1.0,
      tooltip = if (w.name or "") ~= "" then `ws {w.ws} · {w.display or "?"} · {w.windows or 0} windows` else nil,
      onClick = function()
        lib.bsctl({ "ws", "focus", "--ws-id", wsId })
      end,
    }
    if cfg.outline then
      props.border = lib.roleOutline(key)
      props.borderWidth = 1
    else
      props.fill = lib.roleFill(key)
    end
    table.insert(sig, `{wsId}|{role}|{label}|{props.opacity}`)
    table.insert(pills, ui.row(props, children))
  end
  local s = table.concat(sig, ";")
  if s == lastSig then return end
  lastSig = s
  local container = if barWidget.isVertical() then ui.column else ui.row
  barWidget.render(container({ gap = 6, align = "center" }, pills))
end

-- ---- emote engine + refresh ---------------------------------------------------------
function update()
  noctalia.setUpdateInterval(TICK)
  local now = noctalia.nowMs()
  for sid, b in pairs(bots) do
    if b.face and now >= (b.faceUntil or 0) then b.face = nil end
    if b.motion and now - (b.motionAt or 0) >= 360 then b.motion = nil end
    if now >= (b.nextAt or 0) then
      emote(sid, now, false)
      b.nextAt = now + nextGap((bySid[sid] or {}).status)
    end
  end
  render(now)
end

-- Focus change → poke the bots on the newly active workspace (a forced emote).
local function onDisplays(v)
  displays = v or {}
  local d = myDisplay()
  local active = if d then d.activeWs else nil
  if active ~= lastActive then
    if lastActive ~= nil then
      local now = noctalia.nowMs()
      for _, sid in ipairs(sidsByWs[tostring(active)] or {}) do
        emote(sid, now, true)
      end
    end
    lastActive = active
  end
  render(noctalia.nowMs())
end

noctalia.state.watch("workspaces", function(v) workspaces = v or {}; render(noctalia.nowMs()) end)
noctalia.state.watch("displays", onDisplays)
noctalia.state.watch("agents", function(v) applyAgents(v); render(noctalia.nowMs()) end)

function onConfigChanged()
  loadCfg()
  lastSig = ""
  render(noctalia.nowMs())
end

function onRightClick()
  noctalia.togglePanel("battlestation/workspaces:workspaces")
end
