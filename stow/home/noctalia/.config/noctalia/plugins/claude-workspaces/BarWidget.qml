import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Widgets
import qs.Services.Compositor
import qs.Services.UI

Item {
  id: root

  property ShellScreen screen
  property string widgetId: ""
  property string section: ""
  property int sectionWidgetIndex: -1
  property int sectionWidgetsCount: 0
  property var pluginApi: null

  readonly property string screenName: screen ? screen.name : ""
  readonly property real barHeight: Style.getBarHeightForScreen(screenName)
  readonly property real capsuleHeight: Style.getCapsuleHeightForScreen(screenName)
  readonly property real pillSize: 0.7
  readonly property real d: Math.round(capsuleHeight * pillSize)
  readonly property real textRatio: 0.50
  readonly property int characterCount: 20

  readonly property var ps: (pluginApi && pluginApi.pluginSettings) ? pluginApi.pluginSettings : ({})
  readonly property string displayMode: ps.displayMode || "pill"
  readonly property bool outline: ps.outlinePills === true

  implicitWidth: row.implicitWidth + Style.marginS * 2
  implicitHeight: barHeight

  // Per-workspace instance statuses: { "<wsid>": ["green","purple",...] }
  property var instancesByWs: ({})
  // Occupancy computed from real windows (ExtWorkspaceService.isOccupied is unreliable).
  property var occupiedMap: ({})

  // Claude plan usage (from claude-usage.sh).
  readonly property bool showUsage: ps.showUsage === true
  property int sessionPct: -1
  property string sessionResets: ""
  property int weeklyPct: -1
  property string weeklyResets: ""

  // ---- colour helpers -------------------------------------------------------
  function themeKey(status) {
    switch (status) {
    case "green":
      return ps.thinkingColor || "primary";
    case "purple":
      return ps.toolColor || "tertiary";
    case "orange":
      return ps.waitingColor || "error";
    default:
      return "none";
    }
  }
  function customColor(status) {
    switch (status) {
    case "green":
      return ps.thinkingCustom || "#3fb950";
    case "purple":
      return ps.toolCustom || "#a371f7";
    case "orange":
      return ps.waitingCustom || "#c97b47";
    default:
      return "#3a3a3a";
    }
  }
  function resolveKey(key) {
    switch (key) {
    case "primary":
      return Color.mPrimary;
    case "secondary":
      return Color.mSecondary;
    case "tertiary":
      return Color.mTertiary;
    case "error":
      return Color.mError;
    default:
      return Color.mSurfaceVariant;
    }
  }
  function resolveOnKey(key) {
    switch (key) {
    case "primary":
      return Color.mOnPrimary;
    case "secondary":
      return Color.mOnSecondary;
    case "tertiary":
      return Color.mOnTertiary;
    case "error":
      return Color.mOnError;
    default:
      return Color.mOnSurface;
    }
  }
  function contrastOn(hex) {
    const c = Qt.color(hex);
    const lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    return lum > 0.55 ? "#101010" : "#f5f5f5";
  }
  // Colour for a single Claude status (theme key or custom override).
  function statusBg(status) {
    return ps.overrideThemeColors ? customColor(status) : resolveKey(themeKey(status));
  }
  // Claude Code logo tinted per status (icons mode).
  function statusIcon(status) {
    switch (status) {
    case "green":
      return Qt.resolvedUrl("assets/claudecode-thinking.svg");
    case "purple":
      return Qt.resolvedUrl("assets/claudecode-tool.svg");
    case "orange":
      return Qt.resolvedUrl("assets/claudecode-waiting.svg");
    default:
      return Qt.resolvedUrl("assets/claudecode.svg");
    }
  }
  // Eyes-closed frame (the blink).
  function statusIconBlink(status) {
    switch (status) {
    case "green":
      return Qt.resolvedUrl("assets/claudecode-thinking-blink.svg");
    case "purple":
      return Qt.resolvedUrl("assets/claudecode-tool-blink.svg");
    case "orange":
      return Qt.resolvedUrl("assets/claudecode-waiting-blink.svg");
    default:
      return Qt.resolvedUrl("assets/claudecode.svg");
    }
  }

  // Emote frame for a status + expression name (e.g. green + "squint").
  function statusPrefix(status) {
    return status === "green" ? "claudecode-thinking" : status === "purple" ? "claudecode-tool" : "claudecode-waiting";
  }
  function faceUrl(status, emote) {
    return Qt.resolvedUrl("assets/" + statusPrefix(status) + "-" + emote + ".svg");
  }

  // One aggregate status per workspace (priority: tool > thinking > waiting).
  function wsStatus(id) {
    const arr = instancesByWs[String(id)];
    if (!arr || arr.length === 0)
      return "";
    if (arr.indexOf("purple") >= 0)
      return "purple";
    if (arr.indexOf("green") >= 0)
      return "green";
    return "orange";
  }

  // Workspace colour keys: none = transparent, surface = grey, + theme colours.
  function wsKeyColor(key) {
    switch (key) {
    case "grey":
      return Qt.alpha(Color.mOnSurface, 0.22);
    case "primary":
      return Color.mPrimary;
    case "secondary":
      return Color.mSecondary;
    case "tertiary":
      return Color.mTertiary;
    case "error":
      return Color.mError;
    default:
      return "transparent"; // none -> no background
    }
  }
  function wsKeyOn(key) {
    switch (key) {
    case "primary":
      return Color.mOnPrimary;
    case "secondary":
      return Color.mOnSecondary;
    case "tertiary":
      return Color.mOnTertiary;
    case "error":
      return Color.mOnError;
    default:
      return Color.mOnSurface; // none / surface
    }
  }
  function wsKeyFor(role) {
    return role === "focused" ? (ps.focusedColor || "primary") : role === "occupied" ? (ps.occupiedColor || "secondary") : (ps.emptyColor || "grey");
  }
  // Workspace pill background (focused/occupied/empty), theme or custom.
  function wsBg(role) {
    if (ps.overrideThemeColors) {
      if (role === "focused")
        return ps.focusedCustom || "#5e81ac";
      if (role === "occupied")
        return ps.occupiedCustom || "#434c5e";
      return ps.emptyCustom || "#3a3a3a";
    }
    return wsKeyColor(wsKeyFor(role));
  }
  function wsOn(role) {
    if (ps.overrideThemeColors) {
      if (role === "focused")
        return contrastOn(ps.focusedCustom || "#5e81ac");
      if (role === "occupied")
        return contrastOn(ps.occupiedCustom || "#434c5e");
      return contrastOn(ps.emptyCustom || "#3a3a3a");
    }
    return wsKeyOn(wsKeyFor(role));
  }

  function pillColor(ws) {
    if (displayMode !== "icons") {
      const st = wsStatus(ws.id);
      if (st)
        return statusBg(st);
    }
    if (ws.isFocused)
      return wsBg("focused");
    if (occupiedMap[String(ws.id)] === true)
      return wsBg("occupied");
    return wsBg("empty");
  }
  function pillTextColor(ws) {
    if (displayMode !== "icons") {
      const st = wsStatus(ws.id);
      if (st)
        return ps.overrideThemeColors ? contrastOn(customColor(st)) : resolveOnKey(themeKey(st));
    }
    if (ws.isFocused)
      return wsOn("focused");
    if (occupiedMap[String(ws.id)] === true)
      return wsOn("occupied");
    return wsOn("empty");
  }
  // Text colour in outline mode: the pill's own colour (mOnSurface if transparent).
  function pillLineText(ws) {
    const c = pillColor(ws);
    return (c === "transparent") ? Color.mOnSurface : c;
  }

  function pillLabel(ws) {
    return (ws.name && String(ws.name).length > 0) ? String(ws.name).substring(0, characterCount) : String(ws.idx);
  }
  function fmtTime(iso, fmt) {
    if (!iso)
      return "?";
    const dt = new Date(iso);
    return isNaN(dt.getTime()) ? "?" : Qt.formatDateTime(dt, fmt);
  }
  function usageTooltip() {
    return "Session  " + sessionPct + "%   ·   resets " + fmtTime(sessionResets, "HH:mm") + "\n" + "Weekly   " + weeklyPct + "%   ·   resets " + fmtTime(weeklyResets, "ddd d MMM");
  }
  function recomputeOccupancy() {
    var m = {};
    for (var i = 0; i < CompositorService.windows.count; i++) {
      var wid = CompositorService.windows.get(i).workspaceId;
      if (wid !== undefined && wid !== null)
        m[String(wid)] = true;
    }
    occupiedMap = m;
  }

  // ---- pollers --------------------------------------------------------------
  Timer {
    interval: 400
    running: true
    repeat: true
    onTriggered: if (!poller.running)
      poller.running = true
  }
  Process {
    id: poller
    command: ["sh", "-c", "for f in \"$XDG_RUNTIME_DIR\"/claude-ws/*; do [ -e \"$f\" ] && cat \"$f\" && echo; done"]
    stdout: StdioCollector {
      onStreamFinished: {
        var byWs = {};
        var lines = text.split("\n");
        for (var i = 0; i < lines.length; i++) {
          var p = lines[i].trim().split(/\s+/);
          if (p.length === 2) {
            if (!byWs[p[0]])
              byWs[p[0]] = [];
            byWs[p[0]].push(p[1]);
          }
        }
        root.instancesByWs = byWs;
      }
    }
  }
  Timer {
    interval: 500
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.recomputeOccupancy()
  }
  Timer {
    interval: 300000
    running: root.showUsage
    repeat: true
    triggeredOnStart: true
    onTriggered: if (!usageProc.running)
      usageProc.running = true
  }
  Process {
    id: usageProc
    command: ["sh", "-c", "$HOME/.config/hypr/scripts/claude-usage.sh"]
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          const u = JSON.parse(text);
          root.sessionPct = u.sessionPct;
          root.sessionResets = u.sessionResets;
          root.weeklyPct = u.weeklyPct;
          root.weeklyResets = u.weeklyResets;
        } catch (e) {}
      }
    }
  }

  // Right-click anywhere -> context menu (left-clicks fall through to pills).
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.RightButton
    onClicked: PanelService.showContextMenu(contextMenu, root, root.screen)
  }

  Row {
    id: row
    x: Style.marginS
    anchors.verticalCenter: parent.verticalCenter
    spacing: Style.marginXS

    Item {
      id: usageIndicator
      visible: root.showUsage
      width: visible ? usageRow.implicitWidth + Style.marginS : 0
      height: root.barHeight

      Row {
        id: usageRow
        anchors.centerIn: parent
        spacing: Style.marginXXS

        Image {
          anchors.verticalCenter: parent.verticalCenter
          source: Qt.resolvedUrl("assets/claude.svg")
          width: root.d
          height: root.d
          sourceSize.width: Math.round(root.d * 2)
          sourceSize.height: Math.round(root.d * 2)
          fillMode: Image.PreserveAspectFit
          smooth: true
        }
        NText {
          anchors.verticalCenter: parent.verticalCenter
          text: (root.sessionPct >= 0 ? root.sessionPct : "—") + "%"
          pointSize: root.d * root.textRatio
          applyUiScale: false
          color: Color.mOnSurface
        }
      }

      MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        onEntered: {
          if (root.sessionPct >= 0)
            TooltipService.show(usageIndicator, root.usageTooltip(), BarService.getTooltipDirection(root.screenName));
        }
        onExited: TooltipService.hide()
        onCanceled: TooltipService.hide()
      }
    }

    Repeater {
      model: CompositorService.workspaces

      delegate: Item {
        id: cell
        required property var model
        readonly property bool active: model.isFocused === true
        readonly property var instances: root.instancesByWs[String(model.id)] || []

        height: root.barHeight
        width: Math.max(root.d * (active ? 2.2 : 1), Math.round(content.implicitWidth + root.d * (root.outline ? 1.35 : 0.6)))

        Behavior on width {
          NumberAnimation {
            duration: Style.animationNormal
            easing.type: Easing.OutBack
          }
        }

        Rectangle {
          id: pill
          anchors.centerIn: parent
          width: parent.width
          height: root.d
          radius: Style.radiusM
          color: root.outline ? "transparent" : root.pillColor(cell.model)
          border.width: root.outline ? Math.max(2, Math.round(root.d * 0.1)) : 0
          border.color: root.outline ? root.pillColor(cell.model) : "transparent"

          Behavior on color {
            enabled: !Color.isTransitioning
            ColorAnimation {
              duration: Style.animationFast
            }
          }

          Row {
            id: content
            anchors.centerIn: parent
            spacing: Style.marginXXS

            NText {
              anchors.verticalCenter: parent.verticalCenter
              text: root.pillLabel(cell.model)
              family: Settings.data.ui.fontFixed
              pointSize: root.d * root.textRatio
              applyUiScale: false
              font.capitalization: (root.ps.capitalizeNames !== false) ? Font.AllUppercase : Font.MixedCase
              font.weight: cell.active ? Font.Bold : Font.Medium
              color: root.outline ? root.pillLineText(cell.model) : root.pillTextColor(cell.model)
              opacity: cell.active ? 1.0 : 0.7
            }

            // Icons mode: one animated Claude Code bot per instance, tinted by status.
            Repeater {
              model: root.displayMode === "icons" ? cell.instances : []
              delegate: Item {
                id: bot
                required property var modelData
                required property int index
                readonly property string status: bot.modelData
                property string faceOverride: ""
                anchors.verticalCenter: parent.verticalCenter
                width: root.d + 2
                height: root.d

                function rnd(lo, hi) {
                  return lo + Math.random() * (hi - lo);
                }
                // Emote vocabulary per status (weighted by repetition).
                function emotePool() {
                  if (status === "green")
                    return ["bounce", "squint", "look", "happy", "blink", "bounce"];
                  if (status === "purple")
                    return ["wiggle", "wiggle", "surprised", "blink", "bounce"];
                  return ["blink", "sleepy", "look", "bounce"];
                }
                // Cadence: thinking/tools lively, waiting calm.
                function nextDelay() {
                  return status === "orange" ? rnd(20000, 40000) : rnd(500, 1500);
                }
                function showFace(emote, ms) {
                  bot.faceOverride = root.faceUrl(status, emote);
                  faceTimer.interval = ms;
                  faceTimer.restart();
                }
                function performEmote() {
                  // One emote at a time, so a face emote (squint/look) never bounces.
                  if (faceTimer.running || bounceAnim.running || wiggleAnim.running)
                    return;
                  var pool = emotePool();
                  switch (pool[Math.floor(Math.random() * pool.length)]) {
                  case "squint":
                    showFace("squint", 700);
                    break;
                  case "look":
                    showFace("look", 700);
                    break;
                  case "happy":
                    showFace("happy", 700);
                    break;
                  case "surprised":
                    showFace("surprised", 600);
                    break;
                  case "sleepy":
                    showFace("sleepy", 1200);
                    break;
                  case "blink":
                    showFace("blink", 150);
                    break;
                  case "bounce":
                    bounceAnim.restart();
                    break;
                  case "wiggle":
                    wiggleAnim.restart();
                    break;
                  }
                }

                Timer {
                  id: emoteTimer
                  repeat: false
                  onTriggered: {
                    bot.performEmote();
                    interval = bot.nextDelay();
                    start();
                  }
                }
                Timer {
                  id: faceTimer
                  repeat: false
                  onTriggered: bot.faceOverride = ""
                }
                Component.onCompleted: {
                  emoteTimer.interval = bot.nextDelay();
                  emoteTimer.start();
                }

                Image {
                  id: botImg
                  anchors.centerIn: parent
                  source: bot.faceOverride !== "" ? bot.faceOverride : root.statusIcon(bot.status)
                  width: root.d + 2
                  height: root.d + 2
                  sourceSize.width: Math.round(root.d * 2)
                  sourceSize.height: Math.round(root.d * 2)
                  fillMode: Image.PreserveAspectFit
                  smooth: true
                  asynchronous: false
                  transformOrigin: Item.Center
                  transform: Translate {
                    id: bobT
                  }

                  // Bounce emote: a quick double hop (its own emote, not tied to a face).
                  SequentialAnimation {
                    id: bounceAnim
                    NumberAnimation {
                      target: bobT
                      property: "y"
                      from: 0
                      to: -3.5
                      duration: 120
                      easing.type: Easing.OutQuad
                    }
                    NumberAnimation {
                      target: bobT
                      property: "y"
                      from: -3.5
                      to: 0
                      duration: 240
                      easing.type: Easing.OutBounce
                    }
                  }
                  // Wiggle emote: a quick shake.
                  SequentialAnimation {
                    id: wiggleAnim
                    NumberAnimation {
                      target: botImg
                      property: "rotation"
                      from: 0
                      to: -9
                      duration: 60
                    }
                    NumberAnimation {
                      target: botImg
                      property: "rotation"
                      from: -9
                      to: 9
                      duration: 100
                    }
                    NumberAnimation {
                      target: botImg
                      property: "rotation"
                      from: 9
                      to: -6
                      duration: 90
                    }
                    NumberAnimation {
                      target: botImg
                      property: "rotation"
                      from: -6
                      to: 0
                      duration: 70
                    }
                  }
                }
              }
            }
          }
        }

        MouseArea {
          anchors.fill: parent
          cursorShape: Qt.PointingHandCursor
          acceptedButtons: Qt.LeftButton
          onClicked: CompositorService.switchToWorkspace(cell.model)
        }
      }
    }
  }

  NPopupContextMenu {
    id: contextMenu
    model: [
      {
        "label": "Widget Settings",
        "action": "widget-settings",
        "icon": "settings",
        "enabled": true
      }
    ]
    onTriggered: action => {
      contextMenu.close();
      PanelService.closeContextMenu(root.screen);
      if (action === "widget-settings" && pluginApi)
        BarService.openPluginSettings(root.screen, pluginApi.manifest);
    }
  }
}
