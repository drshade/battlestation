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
  // Highest display POSITION that must stay visible (occupied or focused).
  property int maxVisiblePos: 999

  // ---- virtual ordering -----------------------------------------------------
  // Preferred order as real workspace ids (ws.sh's state file). Pills render in
  // this order and are LABELLED BY POSITION, not by Hyprland id. The resolve
  // rule mirrors ws.sh: preferred ids that exist (in order), then any remaining
  // live workspaces ascending. Missing/empty file => identity order.
  property var prefOrder: []
  property var ordered: []  // live workspace objects in resolved display order
  readonly property string orderFilePath: (Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")) + "/claude-workspaces/order"

  function parsePref(txt) {
    prefOrder = String(txt || "").trim().split(/\s+/).map(function (s) {
      return parseInt(s, 10);
    }).filter(function (n) {
      return !isNaN(n);
    });
    recomputeOrder();
  }

  function recomputeOrder() {
    var byId = {};
    var live = [];
    for (var i = 0; i < CompositorService.workspaces.count; i++) {
      var w = CompositorService.workspaces.get(i);
      byId[String(w.id)] = w;
      live.push(w);
    }
    var out = [];
    var seen = {};
    for (var p = 0; p < prefOrder.length; p++) {
      var id = String(prefOrder[p]);
      if (byId[id] !== undefined && seen[id] !== true) {
        out.push(byId[id]);
        seen[id] = true;
      }
    }
    live.sort(function (a, b) {
      return a.id - b.id;
    });
    for (var k = 0; k < live.length; k++) {
      var lid = String(live[k].id);
      if (seen[lid] !== true) {
        out.push(live[k]);
        seen[lid] = true;
      }
    }
    ordered = out;
    recomputeOccupancy();
  }

  function recomputeOccupancy() {
    var m = {};
    for (var i = 0; i < CompositorService.windows.count; i++) {
      var wid = CompositorService.windows.get(i).workspaceId;
      if (wid !== undefined && wid !== null)
        m[String(wid)] = true;
    }
    occupiedMap = m;
    var mx = 1;
    for (var p = 0; p < ordered.length; p++) {
      var w = ordered[p];
      if (m[String(w.id)] === true || w.isFocused === true)
        mx = p + 1;
    }
    maxVisiblePos = mx;
  }

  // Re-resolve the order whenever the workspace set changes or ws.sh (later, a
  // drag) rewrites the preference file.
  Connections {
    target: CompositorService
    function onWorkspacesChanged() {
      root.recomputeOrder();
    }
  }

  FileView {
    id: orderFile
    path: root.orderFilePath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.parsePref(text())
    onLoadFailed: {
      root.prefOrder = [];
      root.recomputeOrder();
    }
  }

  Component.onCompleted: root.recomputeOrder()

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
      model: root.ordered
      delegate: WorkspacePill {
        required property var modelData
        required property int index
        ws: modelData
        position: index + 1
        cfg: config
        instances: root.instancesByWs[String(modelData.id)] || []
        occupied: root.occupiedMap[String(modelData.id)] === true
        shown: !config.hideTrailing || (index + 1) <= root.maxVisiblePos
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
