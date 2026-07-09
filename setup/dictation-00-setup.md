# Dictation (push-to-talk voice typing)

Tap **F12**, speak, tap **F12** again — the transcript is typed into the
focused window. Fully local: `pw-record` → `whisper-cli` (Vulkan, on the
iGPU) → `wtype`. The homegrown `battlestation-dictation` Noctalia plugin (tracked in this
repo) owns the pipeline, the bar status pill, and the mic picker; the
keybind lives in `config/keybinds.lua`.

The stowed config handles the plugin + keybind. Out-of-band steps:

```sh
sudo pacman -S wtype whisper-cpp-vulkan
mkdir -p ~/.local/share/whisper-models
curl -L -o ~/.local/share/whisper-models/ggml-small.en.bin \
  "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"
```

Models are **gitignored** (~466 MB); the plugin defaults to
`~/.local/share/whisper-models/ggml-small.en.bin` (override via the
plugin's `modelPath` setting).

Then: restart Noctalia, add the **Dictation** widget to the bar, and check
the mic in the widget's settings — it follows the system default source
unless one is picked, and a dead default (unplugged interface, gain at
zero) records perfect silence. Sanity-check with:

```sh
timeout 5 pw-record /tmp/mic.wav && ffmpeg -i /tmp/mic.wav -af volumedetect -f null - 2>&1 | grep volume
```

(max_volume near -90 dB = dead source; speech peaks land around -6 dB.)

Verify end-to-end: dictate into a terminal, a browser field, and an agent
prompt; include digits and project jargon (`bsctl`, `Noctalia`) — small.en
mangles some jargon; bump the model if it grates. The keybind is a bare,
modifier-less key **on purpose** — synthetic typing combines with any
physically held modifier in the compositor (digits become SUPER+N
workspace jumps), and modifier-release binds don't fire on this setup.
Recording auto-stops after the plugin's max duration (default 60s).
