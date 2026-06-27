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

  implicitWidth: row.implicitWidth + Style.marginS * 2
  implicitHeight: barHeight

  // { "<workspaceId>": "green" | "purple" | "orange" } — last writer wins
  property var claudeStatus: ({})

  // Plugin settings (theme keys, custom colours, toggles) — set in the pane.
  readonly property var ps: (pluginApi && pluginApi.pluginSettings) ? pluginApi.pluginSettings : ({})

  // Claude plan usage (from claude-usage.sh) — shown when enabled.
  readonly property bool showUsage: ps.showUsage === true
  property int sessionPct: -1
  property string sessionResets: ""
  property int weeklyPct: -1
  property string weeklyResets: ""

  function fmtTime(iso, fmt) {
    if (!iso)
      return "?";
    const dt = new Date(iso);
    return isNaN(dt.getTime()) ? "?" : Qt.formatDateTime(dt, fmt);
  }

  function usageTooltip() {
    return "Session  " + sessionPct + "%   ·   resets " + fmtTime(sessionResets, "HH:mm") + "\n" + "Weekly   " + weeklyPct + "%   ·   resets " + fmtTime(weeklyResets, "ddd d MMM");
  }

  function themeKey(status) {
    switch (status) {
    case "green":
      return ps.thinkingColor || "primary";
    case "purple":
      return ps.toolColor || "tertiary";
    case "orange":
      return ps.waitingColor || "error";
    default:
      return ps.noneColor || "none";
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
      return ps.noneCustom || "#3a3a3a";
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
      return Color.mSurfaceVariant; // "none" / unset -> neutral grey
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

  // Black/white contrast for a custom background colour.
  function contrastOn(hex) {
    const c = Qt.color(hex);
    const lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    return lum > 0.55 ? "#101010" : "#f5f5f5";
  }

  function statusColor(id) {
    const s = claudeStatus[String(id)];
    return ps.overrideThemeColors ? customColor(s) : resolveKey(themeKey(s));
  }

  function statusTextColor(id) {
    const s = claudeStatus[String(id)];
    return ps.overrideThemeColors ? contrastOn(customColor(s)) : resolveOnKey(themeKey(s));
  }

  function pillLabel(ws) {
    return (ws.name && String(ws.name).length > 0) ? String(ws.name).substring(0, characterCount) : String(ws.idx);
  }

  // Mirrors Workspace.qml getWorkspaceWidth: active pill is 2.2x, else fit text.
  function wsWidth(ws, active) {
    const factor = active ? 2.2 : 1;
    const textWidth = pillLabel(ws).length * (d * 0.4);
    const padding = d * 0.6;
    return Style.toOdd(Math.max(d * factor, textWidth + padding));
  }

  Timer {
    interval: 400
    running: true
    repeat: true
    onTriggered: if (!poller.running)
      poller.running = true
  }

  Process {
    id: poller
    command: ["sh", "-c", "for f in \"$XDG_RUNTIME_DIR\"/claude-ws/*; do [ -e \"$f\" ] && printf '%s=%s\\n' \"$(basename \"$f\")\" \"$(cat \"$f\")\"; done"]
    stdout: StdioCollector {
      onStreamFinished: {
        var map = {};
        var lines = text.split("\n");
        for (var i = 0; i < lines.length; i++) {
          var kv = lines[i].split("=");
          if (kv.length === 2 && kv[0].length > 0)
            map[kv[0]] = kv[1];
        }
        root.claudeStatus = map;
      }
    }
  }

  Timer {
    interval: 60000
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

  // Right-click anywhere on the widget -> context menu (left-clicks on pills
  // fall through to their own handlers since this only accepts the right button).
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
          source: Qt.resolvedUrl("assets/claudecode.svg")
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

        height: root.barHeight
        width: root.wsWidth(model, active)

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
          color: root.statusColor(cell.model.id)

          Behavior on color {
            enabled: !Color.isTransitioning
            ColorAnimation {
              duration: Style.animationFast
            }
          }

          NText {
            anchors.fill: parent
            text: root.pillLabel(cell.model)
            family: Settings.data.ui.fontFixed
            pointSize: pill.height * root.textRatio
            applyUiScale: false
            font.capitalization: (root.ps.capitalizeNames !== false) ? Font.AllUppercase : Font.MixedCase
            font.weight: cell.active ? Font.Bold : Font.Medium
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            color: root.statusTextColor(cell.model.id)
            opacity: cell.active ? 1.0 : 0.5

            Behavior on opacity {
              NumberAnimation {
                duration: Style.animationFast
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
