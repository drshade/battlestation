// The status glyph — the whole reason dictation is a plugin and not a bare
// keybind. One BarPill whose colour/text renders Main's state machine:
//
//   idle         dim mic, no text (hidden entirely if showWhenIdle is off)
//   recording    error-red mic, pulsing, elapsed seconds counting up —
//                unmissable, and the stuck-mic tell
//   transcribing tertiary mic + "…" — the busy state that makes the
//                focus-race predictable (text is still in flight)
//   injecting    tertiary mic + "⌨" — wtype is typing right now
//
// Click toggles recording (same as the keybind's toggle).
import QtQuick
import Quickshell
import qs.Commons
import qs.Modules.Bar.Extras
import qs.Services.UI

Item {
  id: root

  property var pluginApi: null
  property ShellScreen screen

  // Widget properties passed from Bar.qml for per-instance settings
  property string widgetId: ""
  property string section: ""
  property int sectionWidgetIndex: -1
  property int sectionWidgetsCount: 0

  readonly property string screenName: screen ? screen.name : ""

  readonly property var main: pluginApi?.mainInstance ?? null
  readonly property string dictState: main?.dictState ?? "idle"

  readonly property var cfg: pluginApi?.pluginSettings || ({})
  readonly property var defaults: pluginApi?.manifest?.metadata?.defaultSettings || ({})
  readonly property bool showWhenIdle: cfg.showWhenIdle ?? defaults.showWhenIdle ?? true

  visible: showWhenIdle || dictState !== "idle"
  implicitWidth: visible ? pill.width : 0
  implicitHeight: pill.height

  // Elapsed recording seconds for the pill text.
  property int elapsedS: 0
  Timer {
    running: root.dictState === "recording"
    interval: 1000
    repeat: true
    triggeredOnStart: true
    onTriggered: root.elapsedS = Math.max(0, Math.round((Date.now() - (root.main?.recordStartedAt || Date.now())) / 1000))
  }
  onDictStateChanged: if (dictState !== "recording") elapsedS = 0

  BarPill {
    id: pill

    screen: root.screen
    oppositeDirection: BarService.getPillDirection(root)
    icon: "microphone"
    // "transparent" is BarPill's own "no override" sentinel (its default).
    customIconColor: root.dictState === "recording" ? Color.mError : root.dictState === "idle" ? "transparent" : Color.mTertiary
    autoHide: false
    text: root.dictState === "recording" ? root.elapsedS + "s" : root.dictState === "transcribing" ? "…" : root.dictState === "injecting" ? "⌨" : ""
    tooltipText: root.dictState === "recording" ? "Recording — tap F12 (or click) to transcribe" : root.dictState === "transcribing" ? "Transcribing…" : root.dictState === "injecting" ? "Typing into the focused window" : "Dictation — tap F12 (or click), speak, tap again"
    onClicked: root.main?.toggle()

    // The recording pulse: opacity breathes so a live mic can't be missed.
    SequentialAnimation on opacity {
      running: root.dictState === "recording"
      loops: Animation.Infinite
      alwaysRunToEnd: true
      NumberAnimation {
        from: 1.0
        to: 0.45
        duration: 500
        easing.type: Easing.InOutQuad
      }
      NumberAnimation {
        from: 0.45
        to: 1.0
        duration: 500
        easing.type: Easing.InOutQuad
      }
    }
  }
}
