// Core shell: owns the per-instance config (Cfg), the live workspace state
// (Claude statuses + window occupancy), and lays out the usage indicator and the
// row of workspace pills. Colours/sizes live in Cfg; pills in WorkspacePill;
// the animated bots in BotIcon.
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

  implicitWidth: row.implicitWidth + Style.marginS * 2
  implicitHeight: config.barHeight

  // Settings + Configuration: colours, sizes, icon URLs.
  Cfg {
    id: config
    pluginApi: root.pluginApi
    screen: root.screen
  }

  // ---- live workspace state -------------------------------------------------
  // Per-workspace instance statuses: { "<wsid>": ["green","purple",...] }
  property var instancesByWs: ({})
  // Occupancy computed from real windows (ExtWorkspaceService.isOccupied is unreliable).
  property var occupiedMap: ({})
  // Highest workspace index that must stay visible (occupied or focused).
  property int maxVisibleIdx: 999

  function recomputeOccupancy() {
    var m = {};
    for (var i = 0; i < CompositorService.windows.count; i++) {
      var wid = CompositorService.windows.get(i).workspaceId;
      if (wid !== undefined && wid !== null)
        m[String(wid)] = true;
    }
    occupiedMap = m;
    var mx = 1;
    for (var j = 0; j < CompositorService.workspaces.count; j++) {
      var w = CompositorService.workspaces.get(j);
      if ((m[String(w.id)] === true || w.isFocused === true) && w.idx > mx)
        mx = w.idx;
    }
    maxVisibleIdx = mx;
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

    UsageIndicator {
      cfg: config
      screenName: config.screenName
    }

    Repeater {
      model: CompositorService.workspaces
      delegate: WorkspacePill {
        required property var model
        ws: model
        cfg: config
        instances: root.instancesByWs[String(model.id)] || []
        occupied: root.occupiedMap[String(model.id)] === true
        shown: !config.hideTrailing || model.idx <= root.maxVisibleIdx
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
