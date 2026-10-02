--!nonstrict
-- The status glyph — the whole reason dictation is a plugin and not a bare
-- keybind. One capsule whose colour/text renders the service's state:
--   idle          dim mic, no text (hidden entirely if show_when_idle is off)
--   recording     error-red mic, pulsing, elapsed seconds — unmissable, the stuck-mic tell
--   transcribing  tertiary mic + "…"  (text still in flight)
--   injecting     tertiary mic + "⌨"  (wtype is typing right now)
-- Click toggles recording (same as the keybind). Right-click: the mic picker.
local dict = noctalia.state.get("dict") or { state = "idle" }
local nonce = 0
local pulse = false

local function send(op)
  nonce += 1
  noctalia.state.set("cmd", { op = op, n = `{noctalia.nowMs()}-{nonce}` })
end

local function render()
  local s = dict.state or "idle"
  local showIdle = noctalia.getConfig("show_when_idle") ~= false
  if s == "idle" and not showIdle then
    barWidget.setVisible(false)
    return
  end
  barWidget.setVisible(true)
  local color = if s == "recording" then "error" elseif s == "idle" then "on_surface_variant" else "tertiary"
  local text = ""
  if s == "recording" then
    text = `{math.max(0, math.floor((noctalia.nowMs() - (dict.startedAt or noctalia.nowMs())) / 1000 + 0.5))}s`
  elseif s == "transcribing" then
    text = "…"
  elseif s == "injecting" then
    text = "⌨"
  end
  barWidget.render(ui.row({ gap = 4, align = "center", opacity = if s == "recording" and pulse then 0.45 else 1.0 }, {
    ui.glyph({ name = "microphone", size = 14, color = color }),
    ui.label({ text = text, fontSize = 12, color = color, visible = text ~= "" }),
  }))
  local tip = if s == "recording" then "Recording — tap F12 (or click) to transcribe"
    elseif s == "transcribing" then "Transcribing…"
    elseif s == "injecting" then "Typing into the focused window"
    else "Dictation — tap F12 (or click), speak, tap again · right-click to pick the mic"
  barWidget.setTooltip(tip)
end

noctalia.state.watch("dict", function(v) dict = v or { state = "idle" }; pulse = false; render() end)

function update()
  if (dict.state or "idle") == "recording" then
    noctalia.setUpdateInterval(500)
    pulse = not pulse
  else
    noctalia.setUpdateInterval(5000)
    pulse = false
  end
  render()
end

function onClick()
  send("toggle")
end

function onRightClick()
  noctalia.togglePanel("battlestation/dictation:mic-picker")
end

function onConfigChanged()
  render()
end
