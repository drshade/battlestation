// Core shell: owns the per-instance config (Cfg), the live workspace state
// (agent statuses + compositor state), and lays out the usage indicator and the
// row of workspace pills. Colours/sizes live in Cfg; pills in WorkspacePill;
// the animated bots in BotIcon.
//
// ONE data source: this widget subscribes to `bsctl status --format json
// --stream` (repo: ctl/src/stream.rs) — the watcher Process below is the only
// state input, one full-world JSON line at start and one per change. No files
// are read or watched, no compositor-service reads, no polling timers: bsctl
// owns every state file and republishes the moment anything we render
// changes. Commands go through bsctl too (focus on click, `ws map set` on
// drag) — the widget is pure presentation.
import QtQuick
import QtQml.Models
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Widgets
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
  // Running subagents, split like the pills' id-sequence + lookup pattern:
  // the ID SEQUENCE drives the sub-bot Repeater (instance-reused unless
  // membership/order changes, so delegates are never rebuilt by property
  // churn), and the tooltip strings flow through a flat map that updates
  // in place. The stream rows also carry `started` (marker mtime, bumped
  // by EVERY tool call the subagent makes) — deliberately dropped here:
  // nothing in the bar renders it, and comparing it used to rebuild the
  // squad — killing any hovered bot's tooltip — on every subagent tool call.
  property var agentsBySid: ({})    // { "<sid>": [agentId, ...] }
  property var subTitleByKey: ({})  // { "<sid>.<agentId>": "<type> — <description>" }

  // ---- compositor state -------------------------------------------------------
  // All of it parsed from the stream's `workspaces`/`displays` sections
  // (schema: ctl/src/lib.rs) by applyState(); plain properties, no live
  // compositor objects. `special:*` workspaces are already excluded by bsctl.
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

  // ---- battlespace order ------------------------------------------------------
  // The stream's `workspaces` array IS the battlespace join, already in bs
  // order (bs N = index N-1, contiguous by construction) — the old QML-side
  // resolve of the order file is gone; bsctl is the single owner of that
  // logic. Pills render in this order and are LABELLED BY BS-ID, not by
  // Hyprland ws-id.
  //
  // Each bar instance shows ONLY the workspaces on its own output (root.screen),
  // but the order and the bs numbers stay GLOBAL -- bs-ids are what
  // SUPER+N / bsctl address, so a pill keeps its global number even when
  // pills before it live on another display. A workspace whose output is
  // unknown fails OPEN (every bar shows it) rather than silently vanishing.
  //
  // Every stream line hands us fresh throwaway JSON, so we keep only
  // plain ids and look everything else up by id. displayList -- the
  // DelegateModel's model -- is rebuilt ONLY when the visible id SEQUENCE
  // changes, so pills (and the bots inside them) survive churn instead of
  // being recreated, which used to reset every bot's breathing/emote timer
  // and freeze animation on a busy workspace.
  property var orderedIds: []   // GLOBAL bs order (all displays), real ws-ids
  property var displayIds: []   // orderedIds filtered to this screen + trimmed: what the ListView shows
  property var displaySlots: [] // displaySlots[k] = 0-based slot of displayIds[k] in orderedIds (bs-id - 1)
  property var displayList: []  // [{id}] stable wrappers keyed by id

  // While a pill is being dragged we freeze displayList so a focus/occupancy
  // event can't reset the DelegateModel mid-gesture. dragIds tracks the live
  // reorder and is what we persist on drop; draggingIndex is its current slot.
  property bool reordering: false
  property var dragIds: []
  property int draggingIndex: -1

  // ---- asks queue -------------------------------------------------------------
  // The stream's `asks` section (file truth — [] when empty, never null): the
  // resolved queue, open asks in the human's order then FIFO, answered tail
  // last. Rows are pushed to the mainInstance so the asks panel (a separate
  // window, recreated per open) binds to live data; the badge in the row
  // below renders the open count. asksKey dedupes on serialization so a
  // stream line that changed something else never churns the panel.
  property var asksRows: []
  property string asksKey: ""
  readonly property int asksOpen: countOpen(asksRows)
  readonly property bool asksAnyBlocking: anyBlockingOpen(asksRows)
  readonly property int asksEstMin: sumEstOpen(asksRows)
  function countOpen(rows) {
    var n = 0;
    for (var i = 0; i < rows.length; i++)
      if (rows[i].state === "open")
        n++;
    return n;
  }
  // The badge's color feed: is any open ask holding an agent's turn open
  // right now (`blocking` — the read-time-sanitized live signal)?
  function anyBlockingOpen(rows) {
    for (var i = 0; i < rows.length; i++)
      if (rows[i].state === "open" && rows[i].blocking === true)
        return true;
    return false;
  }
  function sumEstOpen(rows) {
    var t = 0;
    for (var i = 0; i < rows.length; i++)
      if (rows[i].state === "open" && rows[i].estimate_min)
        t += rows[i].estimate_min;
    return t;
  }
  function applyAsks(rows) {
    var key = JSON.stringify(rows);
    if (key === asksKey)
      return;
    asksKey = key;
    // Enrich each row with its workspace's display name (the stream's ask
    // rows carry only the ws id; names live in the compositor section we
    // already fold into nameById). Panel meta renders "ws 3 — dropkick".
    // An unnamed workspace's name IS its number — suppress the echo.
    for (var i = 0; i < rows.length; i++) {
      var wsKey = rows[i].ws !== null && rows[i].ws !== undefined ? String(rows[i].ws) : "";
      var n = wsKey ? (nameById[wsKey] || "") : "";
      rows[i].wsName = n === wsKey ? "" : n;
    }
    asksRows = rows;
    // Every bar carries identical world state, so last-writer-wins is sound.
    if (pluginApi && pluginApi.mainInstance)
      pluginApi.mainInstance.asksRows = rows;
  }
  function toggleAsksPanel() {
    if (pluginApi && pluginApi.mainInstance)
      pluginApi.mainInstance.toggleAsksPanel(root.screen, root);
  }

  // Equality guards so stream updates only reassign
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
  // Compare one sid's subagent ID sequence. Used to REUSE the prior array
  // instance when membership/order is unchanged, so the pill's sub-bot
  // Repeater (whose model is that instance) is never rebuilt by property
  // churn — type/description ride subTitleByKey and update in place, and
  // volatile `started` is not represented at all (see the property block).
  function sameAgentIds(a, b) {
    if (!a || a.length !== b.length)
      return false;
    for (var i = 0; i < a.length; i++)
      if (a[i] !== b[i])
        return false;
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
    // Persist via bsctl (the map file's only author); the stream then
    // re-emits the world with the new join.
    orderWriter.command = ["sh", "-c", "$HOME/.local/bin/bsctl ws map set " + full.join(" ")];
    orderWriter.running = true;
    // Reflect the new order immediately so there's no flash before the
    // re-emission (orderedIds too, so a stream line can't rebuild from the
    // pre-drag order in the write->re-emit window). The dragged ids keep the
    // same slot SET, so displaySlots stays valid as-is.
    orderedIds = full;
    displayIds = dragIds.slice();
    displayList = dragIds.map(function (id) {
      return {
        "id": id
      };
    });
  }

  Component.onCompleted: {
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

  // ---- state stream -----------------------------------------------------------
  // Event-driven, not polled: `bsctl status --format json --stream` (repo:
  // ctl/src/stream.rs) emits one full-world JSON line immediately and one per
  // change — the world object (protocol spec: ctl/src/lib.rs):
  //   {"displays": [{id, name, x, y, focused, activeWs, specialShowing}],
  //    "workspaces": [{ws, bs, name, display, windows, active, pref}],
  //    "prefs": [{ws, display, present, live}],
  //    "asks": [{id, session, kind, ws, type, title, body, options, urgency,
  //              estimate_min, note, state, answer, created, answered_at,
  //              delivered_at}],
  //    "agents": [{session, kind, status, ws, title,
  //                subagents: [{id, type, description, started}]}]}
  // `agents` is the self-cleaning session pass (dead-pid sessions, orphan
  // markers, kill-leaked stale markers via the transcript-frozen GC);
  // `displays`/`workspaces` are queried fresh per emission (JSON null when
  // Hyprland is unreachable — we keep the previous compositor state). Each
  // bar instance owns its own subscriber process; there is no shared file
  // and no election, so multi-monitor needs no coordination here. `bsctl
  // status` (one-shot) is the debugging view of exactly this payload.
  Process {
    id: watcher
    command: [Quickshell.env("HOME") + "/.local/bin/bsctl", "status", "--format", "json", "--stream"]
    running: true
    stdout: SplitParser {
      splitMarker: "\n"
      onRead: data => root.applyState(data)
    }
  }
  // Respawn guard — sparse, because the subscriber is meant to live forever;
  // this only picks it back up after a crash or a `make build` binary swap
  // (the fresh stream re-emits the world on spawn, so no nudge is needed).
  Timer {
    interval: 5000
    running: true
    repeat: true
    onTriggered: {
      if (!watcher.running)
        watcher.running = true;
    }
  }

  // Parse one stream line: compositor sections first (null displays or
  // workspaces — Hyprland unreachable mid-restart — keep the previous
  // state), then the agents array.
  function applyState(txt) {
    var data;
    try {
      data = JSON.parse(txt);
    } catch (e) {
      data = null;
    }
    if (!data)
      return;
    if (data.workspaces || data.displays)
      applyCompositor(data.workspaces, data.displays);
    // Before the agents early-return: asks are independent of the agents
    // section and must land even on a line without one.
    if (data.asks && data.asks.length !== undefined)
      applyAsks(data.asks);
    var recs = data.agents;
    if (!recs || recs.length === undefined)
      return;
    var statusBySid = {};
    var titleBySid = {};
    var kindBySid = {};
    var agentsBySid = {};
    var subTitleByKey = {};
    var seen = {};      // wsid -> { sid: true } present this poll
    var fresh = {};     // wsid -> [sid] in poll order (for appending new bots)
    for (var i = 0; i < recs.length; i++) {
      var r = recs[i];
      if (!r || !r.session)
        continue;
      var sid = r.session, ws = String(r.ws);
      statusBySid[sid] = r.status || "";
      kindBySid[sid] = r.kind || "claude";
      titleBySid[sid] = r.title || "";
      // Reduce each subagent row to its id (sequence) + tooltip string
      // (lookup); reuse the prior id-list instance when the sequence is
      // unchanged, so the sub-bot Repeater bound to it never sees a new
      // model unless a subagent genuinely started or stopped.
      var list = r.subagents || [];
      var ids = [];
      for (var s = 0; s < list.length; s++) {
        var ag = list[s] || {};
        var aid = String(ag.id !== undefined ? ag.id : s);
        ids.push(aid);
        subTitleByKey[sid + "." + aid] = (ag.type || "agent") + (ag.description ? " — " + ag.description : "");
      }
      var prior = root.agentsBySid[sid];
      agentsBySid[sid] = root.sameAgentIds(prior, ids) ? prior : ids;
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
    // String values, so the plain equality dedupe works; a changed tooltip
    // updates title bindings in place, never rebuilding a delegate. Assigned
    // BEFORE the id sequences so a delegate created by the sequence change
    // never evaluates a not-yet-landed key.
    if (!root.sameKeySet(subTitleByKey, root.subTitleByKey))
      root.subTitleByKey = subTitleByKey;
    // Identity compare is sound here: unchanged id lists were reused above,
    // so a differing value instance means the sequence really changed.
    if (!root.sameKeySet(agentsBySid, root.agentsBySid))
      root.agentsBySid = agentsBySid;
  }

  // Fold the stream's workspaces (the battlespace join, already in bs order)
  // and displays into the plain-data properties, then rebuild the row. Either
  // section can be null alone (partial compositor visibility) — each side
  // keeps its previous state independently. Equality guards keep
  // identical-but-new maps from churning the pills (rebuilding displayList
  // would recreate every bot).
  function applyCompositor(wss, displays) {
    if (wss) {
      var order = [];
      var names = {};
      var outs = {};
      var occ = {};
      for (var i = 0; i < wss.length; i++) {
        var w = wss[i];
        order.push(w.ws);
        names[String(w.ws)] = w.name || "";
        outs[String(w.ws)] = w.display || "";
        if (w.windows > 0)
          occ[String(w.ws)] = true;
      }
      orderedIds = order; // bs order by construction: bs N = index N-1
      if (!sameKeySet(names, nameById))
        nameById = names;
      if (!sameKeySet(outs, outputById))
        outputById = outs;
      if (!sameKeySet(occ, occupiedMap))
        occupiedMap = occ;
    }
    if (displays) {
      // This bar's display = the entry named like root.screen. A missing
      // entry fails OPEN (no highlight suppression, no dimming) like the
      // output filtering above.
      var mine = null;
      var myName = (root.screen && root.screen.name) ? root.screen.name : "";
      for (var m = 0; m < displays.length; m++) {
        if (displays[m].name === myName) {
          mine = displays[m];
          break;
        }
      }
      monActiveId = (mine && mine.activeWs !== null && mine.activeWs !== undefined) ? mine.activeWs : -1;
      specialShowing = !!(mine && mine.specialShowing === true);
      monitorFocused = !mine || mine.focused === true;
    }
    rebuildDisplay();
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

  // Click-to-switch: dispatch through bsctl like every other command — the
  // widget never talks to the compositor directly, for state OR commands.
  function switchToId(id) {
    focusRunner.command = [Quickshell.env("HOME") + "/.local/bin/bsctl", "ws", "focus", "--ws-id", String(id)];
    focusRunner.running = true;
  }

  Process {
    id: focusRunner
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

    AsksBadge {
      cfg: config
      screenName: config.screenName
      count: root.asksOpen
      anyBlocking: root.asksAnyBlocking
      estMin: root.asksEstMin
      onActivated: root.toggleAsksPanel()
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
        subTitleByKey: root.subTitleByKey
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
