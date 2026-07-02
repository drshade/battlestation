// Core shell: owns the per-instance config (Cfg), the live workspace state
// (Claude statuses + window occupancy), and lays out the usage indicator and the
// row of workspace pills. Colours/sizes live in Cfg; pills in WorkspacePill;
// the animated bots in BotIcon.
import QtQuick
import QtQml.Models
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
  // Per-workspace agent instances, keyed by SESSION ID so every bot keeps a
  // stable identity (its own status + title + kind) across polls. Positional
  // arrays would shuffle a title onto the wrong bot the moment a second instance
  // starts or either one stops -- fine when statuses were anonymous, wrong now.
  property var sidsByWs: ({})       // { "<wsid>": [sid, ...] } stable bot order
  property var statusBySid: ({})    // { "<sid>": "thinking" | "tooling" | "waiting" }
  property var titleBySid: ({})     // { "<sid>": "<aiTitle>" }
  property var kindBySid: ({})      // { "<sid>": "claude" } -- future: codex/gemini/...
  property var agentsBySid: ({})    // { "<sid>": [{id,type,description,started}] } running subagents
  // Occupancy computed from real windows (ExtWorkspaceService.isOccupied is unreliable).
  property var occupiedMap: ({})
  // Reactive lookups the pills read by id, so a pill never has to hold a
  // (throwaway) compositor snapshot: names by id, and the focused workspace id.
  property var nameById: ({})
  property int focusedId: -1
  // Highest display POSITION that must stay visible (occupied or focused).
  property int maxVisiblePos: 999

  // ---- virtual ordering -----------------------------------------------------
  // Preferred order as real workspace ids (ws.sh's state file). Pills render in
  // this order and are LABELLED BY POSITION, not by Hyprland id. The resolve
  // rule mirrors ws.sh: preferred ids that exist (in order), then any remaining
  // live workspaces ascending. Missing/empty file => identity order.
  //
  // The compositor rebuilds its workspace ListModel (handing us fresh throwaway
  // row snapshots) on every Hyprland event, so we keep only plain ids and look
  // everything else up by id. displayList -- the DelegateModel's model -- is
  // rebuilt ONLY when the visible id SEQUENCE changes, so pills (and the bots
  // inside them) survive churn instead of being recreated, which used to reset
  // every bot's breathing/emote timer and freeze animation on a busy workspace.
  property var prefOrder: []
  property var orderedIds: []   // resolved order, real ids
  property var displayIds: []   // orderedIds trimmed to what the ListView shows
  property var displayList: []  // [{id}] stable wrappers keyed by id
  readonly property string orderFilePath: (Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")) + "/claude-workspaces/order"

  // While a pill is being dragged we freeze displayList so a focus/occupancy
  // event can't reset the DelegateModel mid-gesture. dragIds tracks the live
  // reorder and is what we persist on drop; draggingIndex is its current slot.
  property bool reordering: false
  property var dragIds: []
  property int draggingIndex: -1

  function parsePref(txt) {
    prefOrder = String(txt || "").trim().split(/\s+/).map(function (s) {
      return parseInt(s, 10);
    }).filter(function (n) {
      return !isNaN(n);
    });
    recomputeOrder();
  }

  function recomputeOrder() {
    var present = {};
    var live = [];
    var names = {};
    for (var i = 0; i < CompositorService.workspaces.count; i++) {
      var w = CompositorService.workspaces.get(i);
      present[String(w.id)] = true;
      live.push(w.id);
      names[String(w.id)] = w.name || "";
    }
    var out = [];
    var seen = {};
    for (var p = 0; p < prefOrder.length; p++) {
      var id = String(prefOrder[p]);
      if (present[id] === true && seen[id] !== true) {
        out.push(prefOrder[p]);
        seen[id] = true;
      }
    }
    live.sort(function (a, b) {
      return a - b;
    });
    for (var k = 0; k < live.length; k++) {
      var lid = String(live[k]);
      if (seen[lid] !== true) {
        out.push(live[k]);
        seen[lid] = true;
      }
    }
    orderedIds = out;
    if (!sameKeySet(names, nameById))
      nameById = names;
    recomputeOccupancy();
  }

  function recomputeOccupancy() {
    var m = {};
    for (var i = 0; i < CompositorService.windows.count; i++) {
      var wid = CompositorService.windows.get(i).workspaceId;
      if (wid !== undefined && wid !== null)
        m[String(wid)] = true;
    }
    if (!sameKeySet(m, occupiedMap))
      occupiedMap = m;
    var fid = -1;
    for (var j = 0; j < CompositorService.workspaces.count; j++) {
      var w = CompositorService.workspaces.get(j);
      if (w.isFocused === true) {
        fid = w.id;
        break;
      }
    }
    if (fid !== focusedId)
      focusedId = fid;
    var mx = 1;
    for (var p = 0; p < orderedIds.length; p++) {
      var oid = orderedIds[p];
      if (m[String(oid)] === true || oid === fid)
        mx = p + 1;
    }
    maxVisiblePos = mx;
    rebuildDisplay();
  }

  // Equality guards so the 400/500 ms pollers only reassign a reactive structure
  // when its content actually changed -- otherwise identical-but-new values churn
  // the consumers (and rebuilding displayList would recreate every pill + bot).
  function sameIntList(a, b) {
    if (!a || !b || a.length !== b.length)
      return false;
    for (var i = 0; i < a.length; i++)
      if (a[i] !== b[i])
        return false;
    return true;
  }
  function sameKeySet(a, b) {
    var ka = Object.keys(a);
    if (ka.length !== Object.keys(b).length)
      return false;
    for (var i = 0; i < ka.length; i++)
      if (b[ka[i]] !== a[ka[i]])
        return false;
    return true;
  }
  function sameInstances(a, b) {
    var ka = Object.keys(a);
    if (ka.length !== Object.keys(b).length)
      return false;
    for (var i = 0; i < ka.length; i++) {
      var k = ka[i], av = a[k], bv = b[k];
      if (!bv || av.length !== bv.length)
        return false;
      for (var j = 0; j < av.length; j++)
        if (av[j] !== bv[j])
          return false;
    }
    return true;
  }
  // Deep-compare one sid's subagent list. Used to REUSE the prior array instance
  // when nothing changed, so the pill's sub-bot Repeater (whose model is that
  // instance) is never rebuilt by a mere re-poll.
  function sameAgentList(a, b) {
    if (!a || a.length !== b.length)
      return false;
    for (var i = 0; i < a.length; i++) {
      var x = a[i], y = b[i];
      if (x.id !== y.id || x.type !== y.type || x.description !== y.description || x.started !== y.started)
        return false;
    }
    return true;
  }

  // Rebuild displayList ONLY when the visible id sequence changes (orderedIds
  // trimmed to maxVisiblePos when hideTrailing). Frozen while reordering so the
  // in-flight drag owns the visual order.
  function rebuildDisplay() {
    if (reordering)
      return;
    var ids = config.hideTrailing ? orderedIds.slice(0, maxVisiblePos) : orderedIds.slice(0);
    if (sameIntList(ids, displayIds))
      return;
    displayIds = ids;
    displayList = ids.map(function (id) {
      return {
        "id": id
      };
    });
  }

  // Drag lifecycle (driven by the ListView delegate).
  function beginReorder(idx) {
    reordering = true;
    draggingIndex = idx;
    dragIds = displayList.map(function (w) {
      return w.id;
    });
  }
  function reorder(from, to) {
    if (from === to || from < 0 || to < 0)
      return;
    var a = dragIds.slice();
    var el = a.splice(from, 1)[0];
    a.splice(to, 0, el);
    dragIds = a;
  }
  // Settled center x (content coords) of the pill at index idx, summed from item
  // WIDTHS (which don't animate) so a swap decision never reacts to an in-flight
  // slide -- that feedback was the back-and-forth flicker.
  function slotCenterX(idx) {
    var x = 0;
    for (var i = 0; i < idx; i++) {
      var it = pillList.itemAtIndex(i);
      x += (it ? it.width : 0) + pillList.spacing;
    }
    var self = pillList.itemAtIndex(idx);
    return x + (self ? self.width : 0) / 2;
  }
  function commitOrder() {
    reordering = false;
    draggingIndex = -1;
    // Persist via ws.sh so the file format has a single author shared with the
    // keybind side; the FileView below then reloads and re-resolves.
    orderWriter.command = ["sh", "-c", "$HOME/.local/bin/ws.sh set " + dragIds.join(" ")];
    orderWriter.running = true;
    // Reflect the new order immediately so there's no flash before the reload.
    displayIds = dragIds.slice();
    displayList = dragIds.map(function (id) {
      return {
        "id": id
      };
    });
  }

  // Re-resolve whenever the workspace set changes or ws.sh/a drag rewrites the file.
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

  Component.onCompleted: {
    root.recomputeOrder();
    // Register this bar so the keybind/IPC rename can attach its panel here.
    if (pluginApi && pluginApi.mainInstance && root.screen)
      pluginApi.mainInstance.registerBar(root.screen.name, root);
  }
  Component.onDestruction: {
    if (pluginApi && pluginApi.mainInstance && root.screen)
      pluginApi.mainInstance.unregisterBar(root.screen.name);
  }

  Process {
    id: orderWriter
  }

  // ---- pollers --------------------------------------------------------------
  Timer {
    interval: 400
    running: true
    repeat: true
    onTriggered: if (!poller.running)
      poller.running = true
  }
  // One flat pass over the claude-ws state dir (claude-ws-status.sh documents the
  // protocol: `<sid>` session JSON + `<sid>.<agent_id>` running-subagent markers).
  // Self-cleaning: a session file whose pid is dead OR that doesn't parse (legacy
  // tab-separated format) is deleted along with its markers, as are orphan markers
  // whose session file is gone. Dotfiles are the writer's in-flight temp files;
  // skip them. Emits ONE JSON array:
  //   [{sid, ws, status, kind, title, agents: [{id, type, description, started}]}]
  // with `started` = the marker's mtime (subagent start time).
  readonly property string pollScript: `
import os, json
d = os.path.join(os.environ.get("XDG_RUNTIME_DIR") or "/tmp", "claude-ws")
try:
    names = os.listdir(d)
except OSError:
    names = []
sess = []
marks = {}
for n in names:
    if n.startswith(".") or n == "debug.log":
        continue
    if "." in n:
        sid, aid = n.split(".", 1)
        marks.setdefault(sid, []).append(aid)
    else:
        sess.append(n)
def rm(p):
    try:
        os.unlink(p)
    except OSError:
        pass
out = []
for sid in sorted(sess):
    p = os.path.join(d, sid)
    try:
        with open(p) as f:
            rec = json.load(f)
        ok = os.path.exists("/proc/%d" % int(rec["pid"]))
    except Exception:
        ok = False
    if not ok:
        rm(p)
        for aid in marks.pop(sid, []):
            rm(p + "." + aid)
        continue
    agents = []
    for aid in marks.pop(sid, []):
        mp = p + "." + aid
        try:
            with open(mp) as f:
                m = json.load(f)
            agents.append({"id": aid, "type": str(m.get("type") or ""), "description": str(m.get("description") or ""), "started": int(os.stat(mp).st_mtime)})
        except Exception:
            pass
    agents.sort(key=lambda a: (a["started"], a["id"]))
    out.append({"sid": sid, "ws": rec.get("ws"), "status": str(rec.get("status") or ""), "kind": str(rec.get("kind") or "claude"), "title": str(rec.get("title") or ""), "agents": agents})
for sid in marks:
    for aid in marks[sid]:
        rm(os.path.join(d, sid + "." + aid))
print(json.dumps(out))
`

  Process {
    id: poller
    command: ["python3", "-c", root.pollScript]
    stdout: StdioCollector {
      onStreamFinished: {
        var recs;
        try {
          recs = JSON.parse(text);
        } catch (e) {
          recs = null;
        }
        if (!recs || recs.length === undefined)
          return;
        var statusBySid = {};
        var titleBySid = {};
        var kindBySid = {};
        var agentsBySid = {};
        var seen = {};      // wsid -> { sid: true } present this poll
        var fresh = {};     // wsid -> [sid] in poll order (for appending new bots)
        for (var i = 0; i < recs.length; i++) {
          var r = recs[i];
          if (!r || !r.sid)
            continue;
          var sid = r.sid, ws = String(r.ws);
          statusBySid[sid] = r.status || "";
          kindBySid[sid] = r.kind || "claude";
          titleBySid[sid] = r.title || "";
          // Reuse the prior list instance when its content is unchanged, so the
          // sub-bot Repeater bound to it never sees a new model on a mere re-poll.
          var list = r.agents || [];
          var prior = root.agentsBySid[sid];
          agentsBySid[sid] = root.sameAgentList(prior, list) ? prior : list;
          if (!seen[ws]) {
            seen[ws] = {};
            fresh[ws] = [];
          }
          if (!seen[ws][sid]) {
            seen[ws][sid] = true;
            fresh[ws].push(sid);
          }
        }
        // Keep each workspace's existing bot order, drop sids that vanished, and
        // append newly-seen ones at the end -- so a status/title change never
        // reshuffles a live bot's slot (only a start/stop touches the sequence).
        var sidsByWs = {};
        for (var ws2 in fresh) {
          var priorSids = root.sidsByWs[ws2] || [];
          var out = [];
          for (var a = 0; a < priorSids.length; a++)
            if (seen[ws2][priorSids[a]])
              out.push(priorSids[a]);
          for (var b = 0; b < fresh[ws2].length; b++)
            if (out.indexOf(fresh[ws2][b]) === -1)
              out.push(fresh[ws2][b]);
          sidsByWs[ws2] = out;
        }
        // sidsByWs is a map of string arrays (sameInstances handles that shape);
        // the value maps churn only when a status/title/kind actually changes.
        if (!root.sameInstances(sidsByWs, root.sidsByWs))
          root.sidsByWs = sidsByWs;
        if (!root.sameKeySet(statusBySid, root.statusBySid))
          root.statusBySid = statusBySid;
        if (!root.sameKeySet(titleBySid, root.titleBySid))
          root.titleBySid = titleBySid;
        if (!root.sameKeySet(kindBySid, root.kindBySid))
          root.kindBySid = kindBySid;
        // Identity compare is sound here: unchanged agent lists were reused above,
        // so a differing value instance means the list's content really changed.
        if (!root.sameKeySet(agentsBySid, root.agentsBySid))
          root.agentsBySid = agentsBySid;
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

  // Right-click on empty bar area -> widget menu (pills handle their own right-click).
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.RightButton
    onClicked: root.showWidgetMenu()
  }

  readonly property var widgetSettingsItem: ({
                                               "label": "Widget Settings",
                                               "action": "widget-settings",
                                               "icon": "settings",
                                               "enabled": true
                                             })

  function showWidgetMenu() {
    contextMenu.model = [widgetSettingsItem];
    PanelService.showContextMenu(contextMenu, root, root.screen);
  }

  // The rename target (wsId/wsName) rides on the menu item itself, so the trigger
  // reads it straight off `item` -- no shared state to be clobbered between the
  // menu opening and the user clicking (e.g. by the other monitor's bar widget).
  function showPillMenu(anchorItem, wsId) {
    contextMenu.model = [{
                           "label": "Rename workspace",
                           "action": "rename",
                           "icon": "edit",
                           "enabled": true,
                           "wsId": wsId,
                           "wsName": root.nameById[String(wsId)] || ""
                         }, widgetSettingsItem];
    PanelService.showContextMenu(contextMenu, root, root.screen, anchorItem);
  }

  // Click-to-switch: look the live workspace up by id (we only keep ids, not the
  // compositor's throwaway snapshots) and hand it to the backend.
  function switchToId(id) {
    for (var i = 0; i < CompositorService.workspaces.count; i++) {
      var w = CompositorService.workspaces.get(i);
      if (w.id === id) {
        CompositorService.switchToWorkspace(w);
        return;
      }
    }
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

    // Pills + a single overlay DropArea. One DropArea over the whole list (rather
    // than one per pill) lets the reflow decide swaps from the cursor position
    // against settled pill centers, so back-and-forth is symmetric and can't
    // oscillate; a DelegateModel slides the others via moveDisplaced.
    Item {
      width: pillList.width
      height: config.barHeight
      anchors.verticalCenter: parent.verticalCenter

      ListView {
        id: pillList
        width: contentWidth
        height: config.barHeight
        orientation: ListView.Horizontal
        interactive: false
        spacing: Style.marginXS
        cacheBuffer: 100000  // keep every delegate realised so reorder never recycles one

        model: DelegateModel {
          id: visualModel
          model: root.displayList
          delegate: dragDelegate
        }

        moveDisplaced: Transition {
          NumberAnimation {
            properties: "x"
            duration: Style.animationFast
            easing.type: Easing.OutQuad
          }
        }
      }

      DropArea {
        anchors.fill: pillList
        onPositionChanged: drag => {
          if (!root.reordering)
            return;
          var hotX = drag.x + pillList.contentX;
          var d = root.draggingIndex;
          var n = pillList.count;
          // Swap with a neighbour only once the cursor passes that neighbour's
          // settled center. After a swap the neighbour moves to the dragged
          // pill's far side, so the reverse threshold is a full pill-width away
          // -- built-in hysteresis, no oscillation.
          if (d + 1 < n && hotX > root.slotCenterX(d + 1)) {
            visualModel.items.move(d, d + 1);
            root.reorder(d, d + 1);
            root.draggingIndex = d + 1;
          } else if (d - 1 >= 0 && hotX < root.slotCenterX(d - 1)) {
            visualModel.items.move(d, d - 1);
            root.reorder(d, d - 1);
            root.draggingIndex = d - 1;
          }
        }
      }
    }
  }

  // One draggable slot. Holds a (visual-only) WorkspacePill; a press+drag floats
  // it above the row, DropAreas on the siblings reorder the model as it crosses
  // them, and release persists the new order.
  Component {
    id: dragDelegate

    MouseArea {
      id: dragArea
      required property var modelData
      required property int index

      readonly property bool dragActive: drag.active

      width: pillVisual.width
      height: pillList.height
      cursorShape: Qt.PointingHandCursor

      acceptedButtons: Qt.LeftButton | Qt.RightButton
      drag.target: pillVisual
      drag.axis: Drag.XAxis
      drag.threshold: 12  // px before a press becomes a drag, so small clicks stay clicks

      // Left switches (suppressed if a drag occurred); right opens the pill menu.
      onClicked: mouse => {
        if (mouse.button === Qt.RightButton)
          root.showPillMenu(dragArea, modelData.id);
        else
          root.switchToId(modelData.id);
      }
      onDragActiveChanged: {
        if (dragActive)
          root.beginReorder(dragArea.DelegateModel.itemsIndex);
        else if (root.reordering)
          root.commitOrder();
      }

      WorkspacePill {
        id: pillVisual
        anchors.verticalCenter: parent.verticalCenter
        wsId: dragArea.modelData.id
        wsName: root.nameById[String(dragArea.modelData.id)] || ""
        focused: dragArea.modelData.id === root.focusedId
        position: dragArea.DelegateModel.itemsIndex + 1  // renumbers live as items move
        cfg: config
        sids: root.sidsByWs[String(dragArea.modelData.id)] || []
        statusBySid: root.statusBySid
        titleBySid: root.titleBySid
        kindBySid: root.kindBySid
        agentsBySid: root.agentsBySid
        occupied: root.occupiedMap[String(dragArea.modelData.id)] === true
        shown: true
        opacity: dragArea.dragActive ? 0.85 : 1.0
        z: dragArea.dragActive ? 1000 : 0

        Drag.active: dragArea.dragActive
        Drag.source: dragArea
        Drag.hotSpot.x: width / 2
        Drag.hotSpot.y: height / 2

        // Float above everything while dragging, reparented to the stable
        // top-level root (not the ListView) so coordinates don't drift as the
        // list reflows. ParentChange preserves position; a plain click (no drag)
        // never activates, so it never reparents.
        states: State {
          when: dragArea.dragActive
          ParentChange {
            target: pillVisual
            parent: root
          }
          AnchorChanges {
            target: pillVisual
            anchors.verticalCenter: undefined
          }
        }
      }
    }
  }

  NPopupContextMenu {
    id: contextMenu
    model: [root.widgetSettingsItem]
    onTriggered: (action, item) => {
      contextMenu.close();
      PanelService.closeContextMenu(root.screen);
      if (action === "rename" && item && pluginApi && pluginApi.mainInstance)
        pluginApi.mainInstance.openRenamePanel(root.screen, root, item.wsId, item.wsName);
      else if (action === "widget-settings" && pluginApi && pluginApi.mainInstance)
        pluginApi.mainInstance.openSettingsPanel(root.screen, root);
    }
  }
}
