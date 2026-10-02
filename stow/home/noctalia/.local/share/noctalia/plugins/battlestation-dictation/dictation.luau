--!nonstrict
-- The dictation state machine (see plugin.toml). Commands arrive from the
-- bar widget via shared state ("cmd") and from the shell via onIpc; the
-- current state is published as "dict" for the widget to render.
local WAV = (noctalia.getenv("XDG_RUNTIME_DIR") or "/tmp") .. "/noctalia-dictation.wav"

local state = "idle" -- idle | recording | transcribing | injecting
local startedAt = 0
local stopRequested = false
local cancelled = false
local lastCmd = 0

local function cfgMax()
  return math.max(5, math.min(58, tonumber(noctalia.getConfig("max_duration_s")) or 55))
end
local function cfgModel()
  local p = noctalia.getConfig("model_path")
  if p == nil or p == "" then
    return noctalia.expandPath("~/.local/share/whisper-models/ggml-small.en.bin")
  end
  return noctalia.expandPath(p)
end
-- Mic node: machine-local state written by the picker panel; "" = system default.
local function micNode()
  local dir = noctalia.pluginDataDir()
  if not dir then return "" end
  local txt = noctalia.readFile(dir .. "/mic.json")
  if not txt then return "" end
  local d = noctalia.json.decode(txt)
  return (d and d.node) or ""
end

local function publish()
  noctalia.state.set("dict", { state = state, startedAt = startedAt })
end
local function setState(s)
  state = s
  publish()
end
local function fail(msg)
  noctalia.notifyError("Dictation", msg)
  setState("idle")
end

local function inject(text)
  setState("injecting")
  -- Text rides argv after `--` (man wtype), so leading-dash text is safe.
  noctalia.runAsync({ "wtype", "--", text }, function(res)
    if cancelled then return setState("idle") end
    if res.exitCode ~= 0 then return fail(`wtype failed (exit {res.exitCode}) — is wtype installed?`) end
    setState("idle")
  end, 20000)
end

local function transcribe()
  setState("transcribing")
  local model = cfgModel()
  local ok = noctalia.runAsync({ "whisper-cli", "-m", model, "-f", WAV, "--no-timestamps", "-np" }, function(res)
    if cancelled then return setState("idle") end
    if res.timedOut then return fail("whisper-cli timed out.") end
    if res.exitCode ~= 0 then return fail(`whisper-cli failed (exit {res.exitCode}) — model at {model}?`) end
    local text = noctalia.string.trim(res.stdout or "")
    -- Non-speech comes back as a bracketed annotation — "(crickets
    -- chirping)", "[BLANK_AUDIO]" — never inject those.
    if text == "" or string.match(text, "^[%[%(][^%]%)]*[%]%)]$") then
      noctalia.notify("Dictation", "Heard no speech.")
      return setState("idle")
    end
    inject(text)
  end, 60000)
  if not ok then fail("Could not start whisper-cli.") end
end

local function start()
  if state ~= "idle" then return end
  stopRequested, cancelled = false, false
  local max = cfgMax()
  -- `timeout -s INT` is the stuck-mic backstop (pw-record finalises the wav
  -- cleanly on SIGINT); the host's own 60s cap stays strictly behind it.
  local argv = { "timeout", "-s", "INT", `{max}s`, "pw-record" }
  local mic = micNode()
  if mic ~= "" then
    table.insert(argv, "--target")
    table.insert(argv, mic)
  end
  table.insert(argv, WAV)
  local ok = noctalia.runAsync(argv, function(res)
    if cancelled then return setState("idle") end
    -- 124 = timeout's own expiry (max duration reached): a normal stop.
    local maxed = res.exitCode == 124
    if not stopRequested and not maxed then
      return fail(`Recorder died (pw-record exit {res.exitCode}) — bad mic target? Right-click the mic to pick one.`)
    end
    if maxed then noctalia.notify("Dictation", "Max duration reached — transcribing.") end
    transcribe()
  end, 60000)
  if not ok then return fail("Could not start pw-record.") end
  startedAt = noctalia.nowMs()
  setState("recording")
end

local function stop()
  if state ~= "recording" then return end
  stopRequested = true
  -- Address the recorder by its unique wav path, never by bare name.
  noctalia.runAsync({ "pkill", "-INT", "-f", "pw-record.*noctalia-dictation\\.wav" }, nil, 5000)
end

local function cancel()
  cancelled = true
  if state == "recording" then
    noctalia.runAsync({ "pkill", "-INT", "-f", "pw-record.*noctalia-dictation\\.wav" }, nil, 5000)
  end
  setState("idle")
end

local function toggle()
  if state == "recording" then stop() else start() end
end

local function dispatch(op)
  if op == "start" then start()
  elseif op == "stop" then stop()
  elseif op == "cancel" then cancel()
  elseif op == "toggle" then toggle() end
end

noctalia.state.watch("cmd", function(v)
  if type(v) == "table" and v.n ~= lastCmd then
    lastCmd = v.n
    dispatch(v.op)
  end
end)

function onIpc(event, _payload)
  dispatch(event)
end

publish()

noctalia.log(`dictation: data dir {tostring(noctalia.pluginDataDir())}, mic {if mic == "" then "(system default)" else mic}`)

-- Say where the mic choice lives, so a silent recording is diagnosable.
local mic = micNode()
noctalia.log(`dictation: data dir {tostring(noctalia.pluginDataDir())}, mic {if mic == "" then "(system default)" else mic}`)
