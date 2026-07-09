// Single plugin instance: the dictation state machine, the pipeline, and the
// IPC entry point the Hyprland push-to-talk keybind calls. The pipeline is
// three direct-argv Processes (no shell anywhere, per the repo rule):
//
//   pw-record --target <mic> <wav>   started on `start`, stopped on `stop`
//   whisper-cli -m <model> -f <wav>  transcribes; stdout is the text
//   wtype -- <text>                  types the text into the focused window
//
// States: idle -> recording -> transcribing -> injecting -> idle. Every
// failure is LOUD (a toast naming the broken piece, per fail-loud-over-
// masking) and returns to idle — there is no wedged state. Two guards:
// a max-duration timer auto-stops recording (with a tap-toggle, forgetting
// to stop IS the normal failure), and stop-vs-died is distinguished
// explicitly (_stopRequested) so a recorder that dies instantly (bad
// --target) fails instead of transcribing nothing.
//
// The keybind is a TAP-TOGGLE on a bare, modifier-less key (F12) by design:
// text typed while a physical modifier is held combines with it in the
// compositor (digits become SUPER+N workspace jumps), and modifier-release
// binds proved undetectable on this setup — see keybinds.lua and git
// history for the Super+\ hold-to-talk attempt this replaces.
//   qs -c noctalia-shell ipc call plugin:dictation toggle
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Services.UI

Item {
  id: root
  property var pluginApi: null

  // ---- the state machine, rendered by BarWidget ----
  // "idle" | "recording" | "transcribing" | "injecting"
  property string dictState: "idle"
  property double recordStartedAt: 0

  property bool _stopRequested: false
  property bool _cancelled: false

  // ---- settings (pluginSettings over manifest defaults) ----
  readonly property var cfg: pluginApi?.pluginSettings || ({})
  readonly property var defaults: pluginApi?.manifest?.metadata?.defaultSettings || ({})
  // "" = the system default source; otherwise a PipeWire node.name (stable
  // across reboots, unlike node ids — the Settings picker stores names).
  readonly property string micNode: cfg.micNode ?? defaults.micNode ?? ""
  readonly property int maxDurationS: cfg.maxDurationS ?? defaults.maxDurationS ?? 60
  readonly property string modelPath: (cfg.modelPath && cfg.modelPath.length > 0) ? cfg.modelPath : Quickshell.env("HOME") + "/.local/share/whisper-models/ggml-small.en.bin"
  // Runtime dir: tmpfs, per-user, gone on reboot — voice audio never sits on disk.
  readonly property string wavPath: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/noctalia-dictation.wav"

  function start() {
    if (dictState !== "idle")
      return;
    _stopRequested = false;
    _cancelled = false;
    var cmd = ["pw-record"];
    if (micNode !== "")
      cmd = cmd.concat(["--target", micNode]);
    cmd.push(wavPath);
    recProc.command = cmd;
    dictState = "recording";
    recordStartedAt = Date.now();
    maxTimer.interval = maxDurationS * 1000;
    maxTimer.restart();
    recProc.running = true;
  }

  function stop() {
    if (dictState !== "recording")
      return;
    maxTimer.stop();
    _stopRequested = true;
    recProc.running = false; // SIGTERM; pw-record finalises the wav cleanly
  }

  function cancel() {
    maxTimer.stop();
    _cancelled = true;
    if (recProc.running)
      recProc.running = false;
    if (whisperProc.running)
      whisperProc.running = false;
    if (wtypeProc.running)
      wtypeProc.running = false;
    dictState = "idle";
  }

  function toggle() {
    dictState === "recording" ? stop() : start();
  }

  function fail(msg) {
    ToastService.showNotice("Dictation", msg, "alert-triangle");
    dictState = "idle";
  }

  // Stuck-mic backstop: bindr doesn't fire if SUPER is released before the
  // key, so a hold that ends "wrong" would record forever without this.
  Timer {
    id: maxTimer
    repeat: false
    onTriggered: {
      ToastService.showNotice("Dictation", "Max duration reached — stopping.", "microphone");
      root.stop();
    }
  }

  Process {
    id: recProc
    onExited: (exitCode, exitStatus) => {
      if (root._cancelled) {
        root.dictState = "idle";
        return;
      }
      if (!root._stopRequested)
        return root.fail("Recorder died (pw-record exit " + exitCode + ") — bad mic target? See Settings.");
      root.dictState = "transcribing";
      whisperProc.command = ["whisper-cli", "-m", root.modelPath, "-f", root.wavPath, "--no-timestamps", "-np"];
      whisperProc.running = true;
    }
  }

  Process {
    id: whisperProc
    stdout: StdioCollector {
      id: whisperOut
    }
    onExited: (exitCode, exitStatus) => {
      if (root._cancelled) {
        root.dictState = "idle";
        return;
      }
      if (exitCode !== 0)
        return root.fail("whisper-cli failed (exit " + exitCode + ") — model at " + root.modelPath + "?");
      var text = (whisperOut.text || "").trim();
      // Non-speech comes back as a bracketed annotation — "(crickets
      // chirping)", "[BLANK_AUDIO]" — never inject those.
      if (text === "" || /^[\[(][^\])]*[\])]$/.test(text)) {
        ToastService.showNotice("Dictation", "Heard no speech.", "microphone");
        root.dictState = "idle";
        return;
      }
      root.inject(text);
    }
  }

  function inject(text) {
    dictState = "injecting";
    wtypeProc.command = ["wtype", "--", text];
    wtypeProc.running = true;
  }

  // Text rides argv after `--` (man wtype: everything past -- is TEXT), so
  // leading-dash text is safe and there is no stdin to close — the
  // write-then-close-stdin approach raced (v1 hung wtype waiting on stdin).
  Process {
    id: wtypeProc
    onExited: (exitCode, exitStatus) => {
      if (root._cancelled) {
        root.dictState = "idle";
        return;
      }
      if (exitCode !== 0)
        return root.fail("wtype failed (exit " + exitCode + ") — is wtype installed?");
      root.dictState = "idle";
    }
  }

  // `qs -c noctalia-shell ipc call plugin:dictation <fn>` — the keybind path.
  IpcHandler {
    target: "plugin:dictation"
    function start() {
      root.start();
    }
    function stop() {
      root.stop();
    }
    function cancel() {
      root.cancel();
    }
    function toggle() {
      root.toggle();
    }
  }
}
