// Core shell: owns the per-instance config (Cfg), the live workspace state
// (Claude statuses + window occupancy), and lays out the usage indicator and the
// row of workspace pills. Colours/sizes live in Cfg; pills in WorkspacePill;
// the animated bots in BotIcon.
import QtQuick
import QtQml.Models
import Quickshell
import Quickshell.Hyprland
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
  // (throwaway) compositor snapshot: names by id, and the highlighted id.
  property var nameById: ({})
  property var outputById: ({})   // monitor name per id ("" = unknown -> fail open)
  // THIS display's active workspace — what the pill highlight shows.
  // Per-screen, not the global focus (each monitor has an active workspace;
  // only the focused monitor's is isFocused). Bound DIRECTLY to Quickshell's
  // reactive Hyprland API rather than CompositorService's rows: the service
  // re-snapshots on raw events / model membership changes, but a workspace
  // moving between monitors only flips `active` PROPERTIES on existing
  // objects, which can settle after the last snapshot — leaving a stale
  // highlight until the next workspace switch (seen live twice after
  // movetodisplay --follow). monitorFor(screen).activeWorkspace notifies the
  // moment the compositor settles, no snapshot in between. (This makes the
  // widget explicitly Hyprland-only, which it already is in practice — every
  // command it runs speaks Hyprland IPC.)
  readonly property var hlMonitor: Hyprland.monitorFor(root.screen)
  readonly property int activeId: (hlMonitor && hlMonitor.activeWorkspace) ? hlMonitor.activeWorkspace.id : -1
  onActiveIdChanged: rebuildDisplay()   // re-evaluate highlight + trailing-trim pinning
  // Whether the keyboard is on THIS monitor — the unfocused display's active
  // pill renders slightly dimmed. Compared via the Hyprland.focusedMonitor
  // singleton (object identity; notifies on focus moves) — the per-monitor
  // `focused` property read false on every monitor here, so it can't be
  // trusted on this Quickshell version.
  readonly property bool monitorFocused: !hlMonitor || Hyprland.focusedMonitor === hlMonitor
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
  // The compositor rebuilds its workspace ListModel (handing us fresh throwaway
  // row snapshots) on every Hyprland event, so we keep only plain ids and look
  // everything else up by id. displayList -- the DelegateModel's model -- is
  // rebuilt ONLY when the visible id SEQUENCE changes, so pills (and the bots
  // inside them) survive churn instead of being recreated, which used to reset
  // every bot's breathing/emote timer and freeze animation on a busy workspace.
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

  function recomputeOrder() {
    // Workspace->output mapping comes from Quickshell's LIVE Hyprland objects,
    // not CompositorService's rows: the rows are snapshots that miss
    // property-only changes (a workspace MOVED between monitors keeps the
    // model's membership, so no re-snapshot fires — seen live as pills left
    // dangling on the old bar after swapdisplays until the next event). The
    // live objects' .monitor is current at read time; the settle timer below
    // re-reads after each burst.
    var liveOuts = {};
    var hws = Hyprland.workspaces ? Hyprland.workspaces.values : [];
    for (var h = 0; h < hws.length; h++)
      liveOuts[String(hws[h].id)] = (hws[h].monitor && hws[h].monitor.name) ? hws[h].monitor.name : "";
    var present = {};
    var live = [];
    var names = {};
    var outs = {};
    for (var i = 0; i < CompositorService.workspaces.count; i++) {
      var w = CompositorService.workspaces.get(i);
      present[String(w.id)] = true;
      live.push(w.id);
      names[String(w.id)] = w.name || "";
      var lo = liveOuts[String(w.id)];
      outs[String(w.id)] = (lo !== undefined) ? lo : (w.output || "");
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
    if (!sameKeySet(outs, outputById))
      outputById = outs;
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
    // activeId is a reactive binding (Hyprland.monitorFor), not computed here.
    // maxVisiblePos is computed in rebuildDisplay, over the FILTERED list.
    rebuildDisplay();
  }

  // Equality guards so state-file reloads and the occupancy poller only reassign
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
      if (occupiedMap[String(vis[p])] === true || vis[p] === activeId)
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
    // (orderedIds too, so the 500ms occupancy tick can't rebuild from the
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

  // Re-resolve whenever the workspace set changes or bsctl ws/a drag rewrites the file.
  Connections {
    target: CompositorService
    function onWorkspacesChanged() {
      // Noctalia's HyprlandService refreshes workspaces + toplevels on every
      // raw event but NEVER monitors, so HyprlandMonitor.activeWorkspace goes
      // stale when a workspace is moved to a monitor without a focus change
      // (seen live: highlight stuck on the destination display's previous
      // workspace). Nudge the monitors refresh ourselves; its async result
      // fires activeWorkspaceChanged and the reactive activeId binding does
      // the rest. Harmless when already current.
      Hyprland.refreshMonitors();
      root.recomputeOrder();
      settleTimer.begin();
    }
  }

  // Trailing re-reads after each event burst: the refresh queries are async,
  // so the last recomputeOrder of a burst (e.g. swapdisplays' 12 dispatches)
  // can run before the final data lands — and a property-only settle fires
  // NO further event to catch it (dangling pills until the next mouse-over;
  // a single 300ms tap was observed losing this race live). So keep
  // re-querying + re-reading every 300ms for up to 10 rounds (~3s) after the
  // last event. Each pass is cheap (pure JS over ~10 workspaces); a new
  // event restarts the schedule.
  Timer {
    id: settleTimer
    property int rounds: 0
    interval: 300
    repeat: true
    onTriggered: {
      Hyprland.refreshWorkspaces();
      Hyprland.refreshMonitors();
      root.recomputeOrder();
      rounds++;
      if (rounds >= 10)
        stop();
    }
    function begin() {
      rounds = 0;
      restart();
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

  // ---- state watcher ----------------------------------------------------------
  // Event-driven, not polled: `bsctl watch` (repo: ctl/src/watch.rs) is a
  // long-lived daemon that inotify-watches the claude-ws state dir and keeps
  // <XDG_RUNTIME_DIR>/claude-ws/.widget.json equal to `bsctl poll`'s output —
  // one flat self-cleaning pass (dead-pid sessions, orphan markers,
  // kill-leaked stale markers via the transcript-frozen GC) emitting ONE JSON
  // array (protocol spec: ctl/src/lib.rs):
  //   [{sid, ws, status, kind, title, agents: [{id, type, description, started}]}]
  // rewritten atomically only when the content changes, plus a 10s tick for
  // what inotify can't see (dying pids, markers aging out). Every bar instance
  // runs one watcher; an exclusive flock makes one the writer and the rest hot
  // standbys that take over if it dies, so multi-monitor needs no coordination
  // here. The FileView reload()s on each write and feeds applyRecs(); `bsctl
  // poll` remains available as a one-shot debugging fallback if watch
  // misbehaves.
  readonly property string stateFilePath: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/claude-ws/.widget.json"
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

  function applyRecs(txt) {
    var recs;
    try {
      recs = JSON.parse(txt);
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
