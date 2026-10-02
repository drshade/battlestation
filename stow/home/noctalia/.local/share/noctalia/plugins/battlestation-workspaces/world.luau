--!nonstrict
-- The world stream. ONE subscriber per shell to `bsctl status --format json
-- --stream` (NDJSON, one full-world line at start and one per change; schema
-- in ctl/src/lib.rs). Each section is republished to the plugin's shared
-- state, where the bar widgets and panels watch it — no polling, no files,
-- no compositor reads anywhere else in this plugin.
--
-- A section that arrives null (Hyprland unreachable mid-restart) keeps its
-- previous value: partial visibility must never blank the bar.
--
-- The subscriber is meant to live forever. runStream has no exit callback,
-- so the shell loop below respawns bsctl if it ever dies (2s backoff); the
-- next line it emits is a full world, so a restart is invisible to readers.
local json = noctalia.json

local lines = 0
-- Toast NEW notify-type asks: an FYI's whole point is being seen, and the
-- badge alone is too quiet. This service is the single dedupe point. The
-- FIRST line seeds the set WITHOUT toasting (a shell restart must not toast
-- the whole standing queue), and only asks that arrive OPEN toast.
local toasted = {}
local seeded = false

local function wsNames(workspaces)
  local names = {}
  for _, w in ipairs(workspaces or {}) do
    names[tostring(w.ws)] = w.name or ""
  end
  return names
end

local function onLine(line)
  lines += 1
  local d, err = json.decode(line)
  if d == nil then
    noctalia.log(`world: bad stream line ({err})`)
    return
  end
  if d.workspaces ~= nil then
    noctalia.state.set("workspaces", d.workspaces)
  end
  if d.displays ~= nil then
    noctalia.state.set("displays", d.displays)
  end
  if d.agents ~= nil then
    local status = {}
    for _, a in ipairs(d.agents) do
      if a.session then
        status[a.session] = a.status or ""
      end
    end
    noctalia.state.set("agents", d.agents)
    noctalia.state.set("statusBySid", status)
  end
  if d.asks ~= nil then
    -- Enrich each row with its workspace's display name. An unnamed
    -- workspace's name IS its number — suppress the echo.
    local names = wsNames(d.workspaces or noctalia.state.get("workspaces"))
    for _, r in ipairs(d.asks) do
      local n = names[tostring(r.ws)] or ""
      r.wsName = if n ~= "" and n ~= tostring(r.ws) then n else ""
      if r.type == "notify" and not toasted[tostring(r.id)] then
        toasted[tostring(r.id)] = true
        if seeded and r.state == "open" then
          noctalia.notify(r.title or "New notify", r.body or "")
        end
      end
    end
    seeded = true
    noctalia.state.set("asks", d.asks)
  end
  if d.comms ~= nil then
    noctalia.state.set("comms", d.comms)
  end
  if d.usage ~= nil then
    noctalia.state.set("usage", d.usage)
  end
  if d.human ~= nil then
    noctalia.state.set("human", d.human)
  end
  noctalia.state.set("world_lines", lines)
end

local bsctl = noctalia.expandPath("~/.local/bin/bsctl")
local cmd = `while :; do "{bsctl}" status --stream --format json; echo "world: bsctl stream exited, respawning" >&2; sleep 2; done`
if not noctalia.runStream(cmd, onLine) then
  noctalia.notifyError("Battlestation", "Could not start the bsctl world stream.")
end
