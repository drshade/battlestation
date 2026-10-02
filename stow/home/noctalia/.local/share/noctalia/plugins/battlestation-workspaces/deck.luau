--!nonstrict
-- The Deck: the asks-queue triage surface (contract: ctl/src/lib.rs, "The
-- asks queue"). Rows bind live to the world stream's `asks`; every action is
-- a direct-argv bsctl call, so human reply text needs no quoting discipline.
--
-- Reply and completion are DECOUPLED: the reply text auto-saves as a draft
-- (`asks reply` — a blocked asker stays parked; agents see the text in
-- progress) 1.5s after the last keystroke and on close — there is no Save
-- button to forget. Done = `asks answer` (bare `asks complete` on an empty
-- input: an ack is an answer); Reopen walks an answered ask back to open
-- keeping its text as a draft. Rows expand on click (one at a time);
-- answered rows expand read-only. Ask TYPES render distinctly (glyph per
-- type beside the live dot), and notify rows — FYIs whose lifecycle is
-- seen → gone — expand to body + "Got it" with no reply machinery.
-- Delivery is tracked, not assumed: an answered row reads "awaiting pickup"
-- until the asker's own collection stamps delivered_at, then "delivered ✓",
-- and the Hide-delivered toggle drops it from the list at that moment.
--
-- Reordering is a drag handle on each open row; release commits the whole
-- open-id order (`asks order set`), anchoring a filtered move onto the
-- global order so hidden rows keep their relative places.
local lib = require("./lib.luau")

local asks = noctalia.state.get("asks") or {}
local statusBySid = noctalia.state.get("statusBySid") or {}

local hideDelivered = noctalia.getConfig("hide_delivered") ~= false
local filterWs = nil -- project filter (ws id) or nil
local expandedId = nil
local drafts = {} -- id -> { text, dirtyAt, persisted }
local deckOn = true
local nowS = os.time()

local DRAG = "deck-ask"

-- ---- queries ------------------------------------------------------------------------
local function byId(id)
  for _, r in ipairs(asks) do
    if r.id == id then return r end
  end
  return nil
end
local function visibleRows()
  local out = {}
  for _, r in ipairs(asks) do
    local hidden = (hideDelivered and r.delivered_at ~= nil) or (filterWs ~= nil and r.ws ~= filterWs)
    if not hidden then table.insert(out, r) end
  end
  return out
end
local function openIds(rows)
  local out = {}
  for _, r in ipairs(rows) do
    if r.state == "open" then table.insert(out, r.id) end
  end
  return out
end
local function counts()
  local open, blocking = 0, 0
  for _, r in ipairs(asks) do
    if r.state == "open" then
      open += 1
      if r.blocking == true then blocking += 1 end
    end
  end
  return open, blocking
end
local function projectChips()
  local seen, out = {}, {}
  for _, r in ipairs(asks) do
    if not (hideDelivered and r.delivered_at ~= nil) and r.ws ~= nil then
      local k = tostring(r.ws)
      if not seen[k] then
        seen[k] = { ws = r.ws, name = r.wsName or "", count = 0 }
        table.insert(out, seen[k])
      end
      seen[k].count += 1
    end
  end
  table.sort(out, function(a, b) return a.ws < b.ws end)
  return out
end

-- Will answering poke the asker's terminal? Only when parked IDLE (waiting)
-- and NOT blocking — a blocking asker's RPC returns the answer directly, a
-- busy asker collects at its next turn. This is the Trigger/Enqueue label.
local function willWake(r)
  return r ~= nil and r.blocking ~= true and statusBySid[r.session] == "waiting"
end

-- ---- actions ---------------------------------------------------------------------------
local function persistDraft(id, force)
  local d = drafts[id]
  if not d or d.persisted == d.text then return end
  if not force and noctalia.nowMs() - (d.dirtyAt or 0) < 1500 then return end
  d.persisted = d.text
  lib.bsctl({ "asks", "reply", id, d.text })
end
local function submitAnswer(id, text)
  local r = byId(id)
  local wake = willWake(r)
  drafts[id] = nil
  if text ~= nil and text ~= "" then
    lib.bsctl({ "asks", "answer", id, text })
  else
    lib.bsctl({ "asks", "complete", id })
  end
  -- A TRIGGER, not a delivery: types "call get_ask N" so the asker fetches
  -- the answer itself; self-guards on a blocking/busy asker.
  if wake then lib.bsctl({ "asks", "wake", id }) end
  expandedId = nil
end
local function dismiss(id)
  if id == expandedId then expandedId = nil end
  drafts[id] = nil
  lib.bsctl({ "asks", "dismiss", id })
end
local function reopen(id)
  lib.bsctl({ "asks", "reopen", id })
end
local function jump(session)
  lib.bsctl({ "ws", "focus", "--session", session })
  panel.close()
end
local function setDeck(on)
  deckOn = on
  lib.bsctl({ "asks", "deck", if on then "on" else "off" })
end
local function loadDeck()
  lib.bsctl({ "asks", "deck" }, function(res)
    local out = string.lower(res.stdout or "")
    if string.find(out, "off", 1, true) then deckOn = false elseif string.find(out, "on", 1, true) then deckOn = true end
    render()
  end)
end

-- Drop: payload = dragged id, value = target index among the VISIBLE open
-- rows. Anchor the visible move onto the global open order.
function onAskDropped(payload, value)
  local id = tonumber(payload)
  local to = tonumber(value)
  local vis = openIds(visibleRows())
  local from = lib.indexOf(vis, id)
  if not id or not to or not from then return end
  table.remove(vis, from)
  if from < to then to -= 1 end
  to = math.max(1, math.min(to, #vis + 1))
  table.insert(vis, to, id)
  local inVis = {}
  for _, x in ipairs(vis) do inVis[x] = true end
  local global = openIds(asks)
  local merged, k = {}, 1
  for _, g in ipairs(global) do
    if inVis[g] then
      table.insert(merged, vis[k]); k += 1
    else
      table.insert(merged, g)
    end
  end
  local args = { "asks", "order", "set" }
  for _, x in ipairs(merged) do table.insert(args, x) end
  lib.bsctl(args)
end

-- ---- presentation --------------------------------------------------------------------------
local typeGlyph = { question = "question-mark", review = "eye", notify = "info-circle" }
local function urgencyColor(u)
  if u == "high" then return "error" elseif u == "medium" then return "primary" end
  return "on_surface_variant"
end
local function urgencyLabel(u)
  if u == "medium" then return "med" end
  return u or "low"
end
local function meta(r)
  -- id first (the handle every verb takes), then who, where, when/how long.
  local parts = { `#{r.id}` }
  if r.blocking == true then table.insert(parts, "blocking — agent waiting") end
  if r.kind then table.insert(parts, r.kind) end
  if r.ws ~= nil then table.insert(parts, `ws {r.ws}` .. (if (r.wsName or "") ~= "" then ` — {r.wsName}` else "")) end
  table.insert(parts, lib.fmtAge(r.created, nowS))
  if r.estimate_min and r.estimate_min > 0 then table.insert(parts, `~{r.estimate_min}m of you`) end
  if r.state == "answered" then
    table.insert(parts, if r.delivered_at then "delivered ✓" else "answered — awaiting pickup")
  elseif r.answer and r.answer ~= "" then
    table.insert(parts, `draft: “{r.answer}”`)
  end
  if r.note and r.note ~= "" then table.insert(parts, `“{r.note}”`) end
  return table.concat(parts, "  ·  ")
end

local function zone(i)
  return ui.dropZone({ key = `gap-{i}`, accepts = { DRAG }, value = tostring(i), onDrop = "onAskDropped",
    height = 3, radius = 4, expandOnDrag = true, hitSlop = 22, fill = "primary/0.12" })
end

local function expandedBody(r)
  local id = r.id
  local kids = {}
  if r.body and r.body ~= "" then
    table.insert(kids, ui.markdown({ key = `body-{id}`, text = r.body }))
  end
  if r.type == "notify" then
    table.insert(kids, ui.row({ justify = "end" }, {
      ui.button({ text = "Got it", variant = "primary", controlSize = "sm", onClick = function() dismiss(id) end }),
    }))
    return ui.column({ key = `exp-{id}`, gap = 8, paddingV = 4, align = "stretch" }, kids)
  end
  if r.state == "answered" then
    table.insert(kids, ui.label({ text = if (r.answer or "") ~= "" then r.answer else "(completed without text)", color = "on_surface_variant", maxLines = 6 }))
    table.insert(kids, ui.row({ gap = 8, justify = "end" }, {
      ui.button({ text = "Reopen", variant = "outline", controlSize = "sm", onClick = function() reopen(id) end }),
      ui.button({ text = "Dismiss", variant = "ghost", controlSize = "sm", onClick = function() dismiss(id) end }),
    }))
    return ui.column({ key = `exp-{id}`, gap = 8, paddingV = 4, align = "stretch" }, kids)
  end
  -- Open question/review: options (one-click answers), the reply draft, actions.
  if type(r.options) == "table" and #r.options > 0 then
    local opts = {}
    for _, o in ipairs(r.options) do
      local text = tostring(o)
      table.insert(opts, ui.button({ text = text, variant = "secondary", controlSize = "sm", onClick = function() submitAnswer(id, text) end }))
    end
    table.insert(kids, ui.row({ gap = 6, align = "center" }, opts))
  end
  local d = drafts[id]
  if not d then
    d = { text = r.answer or "", persisted = r.answer or "" }
    drafts[id] = d
  end
  table.insert(kids, ui.input({
    key = `reply-{id}`, multiline = true, value = d.text, focus = true, height = 84,
    placeholder = "Reply… (Ctrl+Enter answers; the text auto-saves as a draft)",
    onChange = function(v) d.text = v; d.dirtyAt = noctalia.nowMs() end,
    onSubmit = function(v) submitAnswer(id, v) end,
  }))
  local doneLabel = if (d.text or "") == "" then "Done" elseif willWake(r) then "Trigger" else "Enqueue"
  local doneTip = if (d.text or "") == "" then "Complete without text (an ack is an answer)"
    elseif willWake(r) then "Answer and nudge the idle asker to collect it now"
    else "Answer; the asker collects it at its next turn"
  table.insert(kids, ui.row({ gap = 8, align = "center", justify = "space_between" }, {
    ui.row({ gap = 8 }, {
      ui.button({ text = "Jump", glyph = "arrow-up-right", variant = "ghost", controlSize = "sm", tooltip = "Focus the asking session's window", onClick = function() jump(r.session) end }),
      ui.button({ text = "Dismiss", variant = "ghost", controlSize = "sm", onClick = function() dismiss(id) end }),
    }),
    ui.button({ text = doneLabel, variant = "primary", controlSize = "sm", tooltip = doneTip, onClick = function() submitAnswer(id, d.text) end }),
  }))
  return ui.column({ key = `exp-{id}`, gap = 8, paddingV = 4, align = "stretch" }, kids)
end

local function row(r, openIdx)
  local id = r.id
  local isOpen = r.state == "open"
  local dot = if r.blocking == true then "tertiary" else "on_surface/0.35"
  local head = {}
  if isOpen then
    table.insert(head, ui.dragSource({ key = `grip-{id}`, dragType = DRAG, payload = tostring(id), previewAncestor = 2, liftFromLayout = true,
      width = 20, height = 24, align = "center", justify = "center", tooltip = "Drag to reorder" }, {
      ui.glyph({ name = "grip-vertical", size = 14, color = "on_surface_variant" }),
    }))
  else
    table.insert(head, ui.box({ width = 20, height = 24 }))
  end
  table.insert(head, ui.box({ width = 8, height = 8, radius = 4, fill = dot }))
  table.insert(head, ui.glyph({ name = typeGlyph[r.type] or "question-mark", size = 14, color = "on_surface_variant" }))
  table.insert(head, ui.column({ flexGrow = 1, gap = 2 }, {
    ui.label({ text = r.title or "(untitled)", fontWeight = "medium", maxLines = 1 }),
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = urgencyLabel(r.urgency), fontSize = 11, fontWeight = "bold", color = urgencyColor(r.urgency) }),
      ui.label({ text = meta(r), fontSize = 11, color = "on_surface_variant", maxLines = 1, flexGrow = 1 }),
    }),
  }))
  local kids = { ui.row({ key = `head-{id}`, gap = 8, align = "center" }, head) }
  if expandedId == id then table.insert(kids, expandedBody(r)) end
  return ui.column({
    key = `ask-{id}`, gap = 4, padding = 8, radius = 10, align = "stretch",
    fill = if expandedId == id then "surface_variant/0.55" else "surface_variant/0.3",
    onClick = function()
      if expandedId == id then
        persistDraft(id, true)
        expandedId = nil
      else
        if expandedId then persistDraft(expandedId, true) end
        expandedId = id
      end
      render()
    end,
  }, kids)
end

function render()
  local open, blocking = counts()
  local rows = visibleRows()
  local list = {}
  local openIdx = 0
  for _, r in ipairs(rows) do
    if r.state == "open" then
      openIdx += 1
      table.insert(list, zone(openIdx))
    end
    table.insert(list, row(r, openIdx))
  end
  table.insert(list, zone(openIdx + 1))
  if #rows == 0 then
    table.insert(list, ui.label({ text = if #asks == 0 then "The queue is empty." else "Nothing matches the current filter.", color = "on_surface_variant", textAlign = "center" }))
  end

  local chips = { ui.button({ text = "All", variant = if filterWs == nil then "primary" else "ghost", controlSize = "sm", onClick = function() filterWs = nil; render() end }) }
  for _, c in ipairs(projectChips()) do
    local ws = c.ws
    local label = (if c.name ~= "" then c.name else `ws {ws}`) .. ` · {c.count}`
    table.insert(chips, ui.button({ text = label, variant = if filterWs == ws then "primary" else "ghost", controlSize = "sm",
      onClick = function() filterWs = if filterWs == ws then nil else ws; render() end }))
  end

  local subtitle = (if open == 1 then "1 open" else `{open} open`) .. (if blocking > 0 then `  ·  {blocking} blocking` else "")
  panel.render(ui.column({ flexGrow = 1, gap = 10, align = "stretch" }, {
    ui.row({ align = "center", justify = "space_between", gap = 8 }, {
      ui.row({ gap = 10, align = "center" }, {
        ui.label({ text = "The Deck", fontSize = 16, fontWeight = "bold" }),
        ui.label({ text = subtitle, fontSize = 12, color = "on_surface_variant" }),
      }),
      ui.row({ gap = 12, align = "center" }, {
        -- ONE switch for both bsctl steering hooks (`asks deck on|off`): on,
        -- agents are nudged toward the queue; off, they ask inline.
        ui.row({ gap = 6, align = "center", tooltip = "Deck on: agents are steered to post here. Off: they ask inline in their terminals." }, {
          ui.label({ text = "Deck", fontSize = 12, color = "on_surface_variant" }),
          ui.toggle({ checked = deckOn, onChange = function(v) setDeck(v == "true") end }),
        }),
        ui.row({ gap = 6, align = "center" }, {
          ui.label({ text = "Hide delivered", fontSize = 12, color = "on_surface_variant" }),
          ui.toggle({ checked = hideDelivered, onChange = function(v) hideDelivered = v == "true"; render() end }),
        }),
        ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
      }),
    }),
    ui.row({ gap = 6, align = "center" }, chips),
    ui.scroll({ key = "rows", flexGrow = 1, gap = 4, align = "stretch" }, list),
  }))
end

-- ---- lifecycle ----------------------------------------------------------------------------
noctalia.state.watch("asks", function(v) asks = v or {}; render() end)
noctalia.state.watch("statusBySid", function(v) statusBySid = v or {} end)

function onOpen(_context)
  nowS = os.time()
  panel.setWantsSecondTicks(true)
  loadDeck()
  render()
end

function update()
  nowS = os.time()
  for id in pairs(drafts) do persistDraft(id, false) end
  render()
end

function onClose()
  for id in pairs(drafts) do persistDraft(id, true) end
end
