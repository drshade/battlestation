--!nonstrict
-- Shared helpers for every entry: the per-kind presentation registry, bsctl
-- invocation, colour roles and small formatting utilities. Pure functions;
-- no state lives here (state is the `world` service's job).
local M = {}

-- bsctl is deployed to ~/.local/bin (see AGENTS.md); hook environments do
-- not always carry PATH, so address it by absolute path like every caller.
function M.bsctlPath()
  return noctalia.expandPath("~/.local/bin/bsctl")
end

-- Run one bsctl command (direct argv — no shell, so human text needs no
-- quoting). onDone(result) is optional; failures are logged loudly.
function M.bsctl(args, onDone)
  local argv = { M.bsctlPath() }
  for _, a in ipairs(args) do
    table.insert(argv, tostring(a))
  end
  local accepted = noctalia.runAsync(argv, function(res)
    if res.exitCode ~= 0 then
      noctalia.log(`bsctl {table.concat(args, " ")} failed ({res.exitCode}): {res.stderr}`)
    end
    if onDone then
      onDone(res)
    end
  end, 15000)
  if not accepted then
    noctalia.log(`bsctl {table.concat(args, " ")}: runAsync refused`)
  end
  return accepted
end

-- ---- per-kind (harness) presentation registry ------------------------------
-- THE single point of definition for how an agent kind looks. Adding a
-- harness = one entry here + an assets/<dir>/ with the files it names:
-- icon.svg (brand mark), <status>.svg per status, <status>-<face>.svg per
-- face. An unknown kind renders as claude rather than breaking.
--   label    tooltip name while a session has no title
--   neutral  stem rendered for an unknown/empty status
--   rest     face a commander (bot with subagents) rests on; "" = status icon
--   faces    per status: face-swap emotes the picker may draw
--   motions  per status: size/flicker emotes (kind-agnostic mechanics in pills)
-- A status's pool = faces[status] ++ motions[status]; repeats are weights.
M.kinds = {
  claude = {
    label = "Claude Code",
    dir = "claude",
    neutral = "base",
    rest = "look",
    faces = { thinking = { "squint", "look", "happy", "blink" }, tooling = { "surprised", "blink" }, waiting = { "blink", "sleepy", "look" } },
    motions = { thinking = { "bounce", "bounce" }, tooling = { "wiggle", "wiggle", "bounce" }, waiting = { "bounce" } },
  },
  codex = {
    label = "Codex",
    dir = "codex",
    neutral = "waiting",
    rest = "",
    faces = { thinking = { "spark", "spark" }, tooling = { "bracket", "spark" }, waiting = { "pause", "pause" } },
    motions = { thinking = { "bounce" }, tooling = { "wiggle", "wiggle", "bounce" }, waiting = { "bounce" } },
  },
  agy = {
    label = "Antigravity",
    dir = "agy",
    neutral = "waiting",
    rest = "",
    faces = { thinking = { "drift", "drift" }, tooling = { "orbit", "spark" }, waiting = { "float", "float" } },
    motions = { thinking = { "bounce" }, tooling = { "wiggle", "wiggle", "bounce" }, waiting = { "bounce" } },
  },
}

-- Protocol statuses are semantic; the on-disk stems are the asset families.
M.statusStem = { thinking = "thinking", tooling = "tool", waiting = "waiting" }

-- How long a face emote holds before the bot returns to its status icon.
M.faceHold = {
  blink = 150, squint = 700, look = 700, happy = 700, surprised = 600, sleepy = 1200,
  spark = 600, bracket = 700, pause = 1200, drift = 700, orbit = 700, float = 1200,
}

function M.kindDef(kind)
  return M.kinds[kind] or M.kinds.claude
end
function M.kindLabel(kind)
  return M.kindDef(kind).label
end
function M.kindIcon(kind)
  return `assets/{M.kindDef(kind).dir}/icon.svg`
end
function M.statusIcon(kind, status)
  local def = M.kindDef(kind)
  return `assets/{def.dir}/{M.statusStem[status] or def.neutral}.svg`
end
function M.faceIcon(kind, status, face)
  return `assets/{M.kindDef(kind).dir}/{M.statusStem[status] or "waiting"}-{face}.svg`
end
function M.restIcon(kind, status)
  local def = M.kindDef(kind)
  if def.rest ~= "" then
    return M.faceIcon(kind, status, def.rest)
  end
  return M.statusIcon(kind, status)
end
function M.emotePool(kind, status)
  local def = M.kindDef(kind)
  local key = if M.statusStem[status] then status else "waiting"
  local pool = {}
  for _, f in ipairs(def.faces[key]) do table.insert(pool, f) end
  for _, m in ipairs(def.motions[key]) do table.insert(pool, m) end
  return pool
end

-- ---- colour roles ------------------------------------------------------------
-- Settings name palette roles; "grey" is on_surface at 22%, "none" is no fill.
function M.roleFill(key)
  if key == "grey" then return "on_surface/0.22" end
  if key == "none" or key == nil or key == "" then return nil end
  return key
end
function M.roleOn(key)
  if key == "grey" or key == "none" or key == nil or key == "" then return "on_surface" end
  return "on_" .. key
end
function M.roleOutline(key)
  if key == "grey" then return "on_surface/0.45" end
  if key == "none" or key == nil or key == "" then return nil end
  return key
end

-- ---- formatting ----------------------------------------------------------------
function M.capitalize(s)
  if s == nil or s == "" then return s end
  return string.upper(string.sub(s, 1, 1)) .. string.sub(s, 2)
end

-- ISO-8601 ("2026-10-02T21:09:59.92+00:00" / "...Z") -> unix seconds (UTC). nil on junk.
function M.isoToEpoch(iso)
  if type(iso) ~= "string" then return nil end
  local y, mo, d, h, mi, s = string.match(iso, "^(%d+)%-(%d+)%-(%d+)T(%d+):(%d+):(%d+)")
  if not y then return nil end
  local sign, oh, om = string.match(iso, "([%+%-])(%d%d):(%d%d)$")
  y, mo, d, h, mi, s = tonumber(y), tonumber(mo), tonumber(d), tonumber(h), tonumber(mi), tonumber(s)
  -- days from civil (Howard Hinnant), proleptic Gregorian
  local yy = if mo <= 2 then y - 1 else y
  local era = (if yy >= 0 then yy else yy - 399) // 400
  local yoe = yy - era * 400
  local mp = (mo + 9) % 12
  local doy = (153 * mp + 2) // 5 + d - 1
  local doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
  local days = era * 146097 + doe - 719468
  local epoch = days * 86400 + h * 3600 + mi * 60 + s
  if sign then
    local off = tonumber(oh) * 3600 + tonumber(om) * 60
    epoch = if sign == "+" then epoch - off else epoch + off
  end
  return epoch
end

function M.fmtAge(createdS, nowS)
  if not createdS then return "" end
  local s = math.max(0, math.floor(nowS - createdS + 0.5))
  if s < 60 then return `{s}s` end
  if s < 3600 then return `{math.floor(s / 60 + 0.5)}m` end
  return `{s // 3600}h`
end

function M.indexOf(list, v)
  for i, x in ipairs(list) do
    if x == v then return i end
  end
  return nil
end

return M
