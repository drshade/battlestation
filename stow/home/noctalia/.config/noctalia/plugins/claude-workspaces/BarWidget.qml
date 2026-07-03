// Core shell: owns the per-instance config (Cfg), the live workspace state
// (agent statuses + compositor state), and lays out the usage indicator and the
// row of workspace pills. Colours/sizes live in Cfg; pills in WorkspacePill;
// the animated bots in BotIcon.
//
// ONE data source: agent hooks and the compositor both flow through
// `bsctl watch` (repo: ctl/src/watch.rs), which folds them into
// <XDG_RUNTIME_DIR>/battlestation-ws/.widget.json; the stateFile FileView
// below is the only state input. No compositor-service reads, no polling
// timers, no staleness workarounds — the daemon subscribes to Hyprland's
// event socket and rewrites the file the moment anything we render changes.
// (CompositorService remains solely as the switch-workspace COMMAND boundary;
// the order file keeps its own FileView — a different protocol, bsctl ws's.)
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

  // ---- compositor state -------------------------------------------------------
  // All of it parsed from .widget.json's `compositor` section (schema:
  // ctl/src/lib.rs) by applyCompositor(); plain properties, no live
  // compositor objects. `special:*` workspaces are already excluded by watch.
  property var liveIds: []          // live workspace ids, unordered
  // Reactive lookups the pills read by id, so a pill never has to hold a
  // (throwaway) snapshot: names/outputs/occupancy by id.
  property var nameById: ({})
  property var outputById: ({})     // monitor name per id ("" = unknown -> fail open)
  property var occupiedMap: ({})    // windows > 0
  // THIS display's active workspace — what the pill highlight shows.
  // Per-screen, not the global focus (each monitor has an active workspace;
  // only the focused monitor's is `focused`). Also used for trailing-trim
  // pinning even while the scratchpad covers it (the workspace hasn't gone
  // anywhere). -1 when this screen has no compositor.monitors entry.
  property int monActiveId: -1
  // A special workspace (scratchpad) showing on this monitor: no pill is
  // "current", so the highlight clears.
  property bool specialShowing: false
  // What the pill highlight compares against (-1 while the scratchpad is up).
  readonly property int activeId: specialShowing ? -1 : monActiveId
  // Whether the keyboard is on THIS monitor — the unfocused display's active
  // pill renders slightly dimmed. Straight from the compositor's own
  // monitors[].focused (missing entry fails open to focused/undimmed).
  property bool monitorFocused: true
  // Highest FILTERED position that must stay visible (occupied or active).
  property int maxVisiblePos: 999

  // ---- virtual ordering -----------------------------------------------------
  // Preferred order as real workspace ids (bsctl ws's state file). Pills render in
  // this order and are LABELLED BY POSITION, not by Hyprland id. The resolve
  // rule mirrors bsctl ws: preferred ids that exist (in order), then any remaining
  // live workspaces ascending. Missing/empty file => identity order.
  //
  // Each bar instance shows ONLY the workspaces on its own output (root.screen),
  // but the order and the position numbers stay GLOBAL -- positions are what
  // SUPER+N / bsctl ws address, so a pill keeps its global number even when
  // pills before it live on another display. A workspace whose output is
  // unknown fails OPEN (every bar shows it) rather than silently vanishing.
  //
  // Every state-file reload hands us fresh throwaway JSON, so we keep only
  // plain ids and look everything else up by id. displayList -- the
  // DelegateModel's model -- is rebuilt ONLY when the visible id SEQUENCE
  // changes, so pills (and the bots inside them) survive churn instead of
  // being recreated, which used to reset every bot's breathing/emote timer
  // and freeze animation on a busy workspace.
  property var prefOrder: []
  property var orderedIds: []   // resolved GLOBAL order (all displays), real ids
  property var displayIds: []   // orderedIds filtered to this screen + trimmed: what the ListView shows
  property var displaySlots: [] // displaySlots[k] = 0-based slot of displayIds[k] in orderedIds (its global position)
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

  // Resolve orderedIds from prefOrder against the compositor's live ids
  // (both plain data — prefOrder from the order file, liveIds from the state
  // file), then rebuild the visible row.
  function recomputeOrder() {
    var present = {};
    for (var i = 0; i < liveIds.length; i++)
      present[String(liveIds[i])] = true;
    var out = [];
    var seen = {};
    for (var p = 0; p < prefOrder.length; p++) {
      var id = String(prefOrder[p]);
      if (present[id] === true && seen[id] !== true) {
        out.push(prefOrder[p]);
        seen[id] = true;
      }
    }
    var live = liveIds.slice().sort(function (a, b) {
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
    // maxVisiblePos is computed in rebuildDisplay, over the FILTERED list.
    rebuildDisplay();
  }

  // Equality guards so state-file reloads only reassign
  // a reactive structure when its content actually changed -- otherwise
  // identical-but-new values churn the consumers (and rebuilding displayList
  // would recreate every pill + bot).
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

  // Rebuild displayList ONLY when the visible id sequence changes. Visible =
  // orderedIds filtered to this screen's output (unknown output fails open),
  // then -- when hideTrailing -- trimmed after the last occupied-or-focused
  // FILTERED position, so this bar never reserves space for another display's
  // trailing empties. displaySlots carries each visible id's global slot for
  // the position labels; it can change alone (another display reordered around
  // us) and is then reassigned WITHOUT rebuilding displayList, so the pills
  // relabel in place instead of being recreated. Frozen while reordering so
  // the in-flight drag owns the visual order.
  function rebuildDisplay() {
    if (reordering)
      return;
    var mine = (root.screen && root.screen.name) ? root.screen.name : "";
    var vis = [];
    var slots = [];
    for (var i = 0; i < orderedIds.length; i++) {
      var out = outputById[String(orderedIds[i])];
      if (!out || !mine || out === mine) {
        vis.push(orderedIds[i]);
        slots.push(i);
      }
    }
    var mx = 1;
    for (var p = 0; p < vis.length; p++)
      if (occupiedMap[String(vis[p])] === true || vis[p] === monActiveId)
        mx = p + 1;
    maxVisiblePos = mx;
    if (config.hideTrailing) {
      vis = vis.slice(0, mx);
      slots = slots.slice(0, mx);
    }
    if (!sameIntList(slots, displaySlots))
      displaySlots = slots;
    if (sameIntList(vis, displayIds))
      return;
    displayIds = vis;
    displayList = vis.map(function (id) {
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
    // dragIds holds only THIS screen's visible pills, but the order file is
    // GLOBAL -- persisting it raw would clobber every other display's slots.
    // Splice instead: keep each id that wasn't part of the drag in its current
    // global slot, and pour the dragged ids, in their new sequence, into the
    // slots that set occupies in the global order. E.g. global [3,1,10,2] with
    // [3,10] shown here, dragged to [10,3]: slots 0 and 2 receive 10 then 3,
    // so we write [10,1,3,2].
    var inDrag = {};
    for (var i = 0; i < dragIds.length; i++)
      inDrag[String(dragIds[i])] = true;
    var full = orderedIds.slice();
    var k = 0;
    for (var s = 0; s < full.length; s++)
      if (inDrag[String(full[s])] === true)
        full[s] = dragIds[k++];
    // Persist via bsctl ws so the file format has a single author shared with the
    // keybind side; the FileView below then reloads and re-resolves.
    orderWriter.command = ["sh", "-c", "$HOME/.local/bin/bsctl ws set " + full.join(" ")];
    orderWriter.running = true;
    // Reflect the new order immediately so there's no flash before the reload
    // (orderedIds too, so a state-file reload can't rebuild from the
    // pre-drag order in the write->inotify window). The dragged ids keep the
    // same slot SET, so displaySlots stays valid as-is.
    orderedIds = full;
    displayIds = dragIds.slice();
    displayList = dragIds.map(function (id) {
      return {
        "id": id
      };
    });
  }

  // The order file: bsctl ws's protocol, re-resolved whenever a keybind or a
  // drag rewrites it. (Workspace-set changes arrive through the state file.)
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

  // ---- state watcher ----------------------------------------------------------
  // Event-driven, not polled: `bsctl watch` (repo: ctl/src/watch.rs) is a
  // long-lived daemon that inotify-watches the battlestation-ws state dir AND
  // subscribes to Hyprland's .socket2.sock event stream, folding both into
  // <XDG_RUNTIME_DIR>/battlestation-ws/.widget.json — one JSON object
  // (protocol spec: ctl/src/lib.rs):
  //   {"sessions": [{sid, ws, status, kind, title,
  //                  agents: [{id, type, description, started}]}],
  //    "compositor": {"workspaces": [{id, name, monitor, windows}],
  //                   "monitors": [{name, x, y, focused, activeWs,
  //                                 specialShowing}]}}
  // `sessions` is the self-cleaning poll pass (dead-pid sessions, orphan
  // markers, kill-leaked stale markers via the transcript-frozen GC);
  // `compositor` is queried fresh per recompute (null when Hyprland is
  // unreachable — we keep the previous compositor state). Rewritten
  // atomically only when the content changes, plus a 10s tick for what
  // inotify can't see (dying pids, markers aging out). Every bar instance
  // runs one watcher; an exclusive flock makes one the writer and the rest hot
  // standbys that take over if it dies, so multi-monitor needs no coordination
  // here. The FileView reload()s on each write and feeds applyRecs(); `bsctl
  // poll` remains available as a one-shot debugging fallback (sessions
  // array only, by contract) if watch misbehaves.
  readonly property string stateFilePath: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/battlestation-ws/.widget.json"
  property bool stateEverLoaded: false

  Process {
    id: watcher
    command: [Quickshell.env("HOME") + "/.local/bin/bsctl", "watch"]
    running: true
  }
  // Respawn guard — sparse, because the daemon is meant to live forever; this
  // only picks it back up after a crash or a `make build` binary swap. Also
  // nudges the FileView until its first successful load: the file may not
  // exist yet while the daemon is starting (missing file = no update, wait).
  Timer {
    interval: 5000
    running: true
    repeat: true
    onTriggered: {
      if (!watcher.running)
        watcher.running = true;
      if (!root.stateEverLoaded)
        stateFile.reload();
    }
  }

  FileView {
    id: stateFile
    path: root.stateFilePath
    watchChanges: true
    printErrors: false // missing until the daemon's first write — not an error
    onFileChanged: reload()
    onLoaded: {
      root.stateEverLoaded = true;
      root.applyRecs(text());
    }
  }

  // Parse one .widget.json payload: compositor section first (a null
  // compositor — Hyprland unreachable mid-restart — keeps the previous
  // compositor state), then the sessions array.
  function applyRecs(txt) {
    var data;
    try {
      data = JSON.parse(txt);
    } catch (e) {
      data = null;
    }
    if (!data)
      return;
    if (data.compositor)
      applyCompositor(data.compositor);
    var recs = data.sessions;
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
      // sub-bot Repeater bound to it never sees a new model on a mere reload.
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

  // Fold the compositor section into the plain-data properties, then
  // re-resolve the row. Equality guards keep identical-but-new maps from
  // churning the pills (rebuilding displayList would recreate every bot).
  function applyCompositor(c) {
    var wss = c.workspaces || [];
    var live = [];
    var names = {};
    var outs = {};
    var occ = {};
    for (var i = 0; i < wss.length; i++) {
      var w = wss[i];
      live.push(w.id);
      names[String(w.id)] = w.name || "";
      outs[String(w.id)] = w.monitor || "";
      if (w.windows > 0)
        occ[String(w.id)] = true;
    }
    liveIds = live;
    if (!sameKeySet(names, nameById))
      nameById = names;
    if (!sameKeySet(outs, outputById))
      outputById = outs;
    if (!sameKeySet(occ, occupiedMap))
      occupiedMap = occ;
    // This bar's monitor = the entry named like root.screen. A missing entry
    // fails OPEN (no highlight suppression, no dimming) like the output
    // filtering above.
    var mine = null;
    var mons = c.monitors || [];
    var myName = (root.screen && root.screen.name) ? root.screen.name : "";
    for (var m = 0; m < mons.length; m++) {
      if (mons[m].name === myName) {
        mine = mons[m];
        break;
      }
    }
    monActiveId = (mine && mine.activeWs !== null && mine.activeWs !== undefined) ? mine.activeWs : -1;
    specialShowing = !!(mine && mine.specialShowing === true);
    monitorFocused = !mine || mine.focused === true;
    recomputeOrder();
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

  // Click-to-switch: look the live workspace up by id and hand it to the
  // backend. The one remaining CompositorService use — a COMMAND, not state;
  // everything rendered comes from .widget.json.
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
        focused: dragArea.modelData.id === root.activeId
        dimFocus: !root.monitorFocused
        // GLOBAL position of the slot this pill sits in (labels stay the
        // SUPER+N numbers even though the row is filtered to this screen).
        // Renumbers live during a drag: itemsIndex moves while displaySlots is
        // frozen, so each pill shows the global slot it would land on.
        position: {
          var s = root.displaySlots[dragArea.DelegateModel.itemsIndex];
          return (s !== undefined ? s : dragArea.DelegateModel.itemsIndex) + 1;
        }
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
