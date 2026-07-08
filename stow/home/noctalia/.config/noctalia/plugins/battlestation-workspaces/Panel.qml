// Bar-attached plugin panel, used for three things so all share the same
// hang-off-the-bar blob, animation and exclusive keyboard:
//   mode "rename"   -> a single text field to rename a workspace
//   mode "settings" -> the widget's Settings.qml hosted with Apply/Close
//   mode "asks"     -> the asks-queue triage surface (rows bind live to
//                      mainInstance.asksRows, which the bar's stream pushes)
// The mode and the rename target are staged on the plugin's mainInstance before
// the panel is opened (the panel content is recreated on every open). Every
// asks action is a direct-argv bsctl Process — no shell anywhere, so human
// reply text needs no quoting discipline at all.
//
// Reply and completion are decoupled (the asks contract in ctl/src/lib.rs):
// the reply text AUTO-SAVES as a draft (`asks reply` — a blocked asker
// stays parked; agents see the text in progress) on a 1.5s typing debounce,
// on blur, on collapse and on panel close — there is no Save button to
// forget. Done = `asks answer` (or bare `asks complete` when the input is
// empty — an ack is an answer), Reopen walks an answered ask back to open
// with its text kept as a draft. Rows expand on CLICK anywhere in the row
// body (single expansion, one ask at a time); answered rows expand to a
// read-only view of their answer. Ask TYPES render distinctly: a glyph per
// type (question-mark / eye / info-circle) beside the urgency dot, and
// notify rows — FYIs whose lifecycle is seen -> gone — expand to body +
// "Got it" (dismiss) with no reply machinery at all; their creation also
// toasts (Main.qml owns that, as the single dedupe point across bars).
// Delivery is tracked, not assumed: an
// answered row reads "awaiting pickup" until the asker's own MCP
// collection stamps delivered_at, then "delivered ✓" — and the
// default-on "Hide delivered" checkbox drops it from the list the moment
// that happens (the auto-fade; persisted in pluginSettings).
//
// Reordering is a drag HANDLE (the grip at each open row's left edge), not a
// full-row drag: row bodies keep their clicks, and pressing the handle first
// collapses the expanded reply area so every open row has the same height.
// The rows live in a ListView + DelegateModel (v2vm), so the drag ANIMATES —
// as the pointer crosses a neighbour's settled centre the grip calls
// `v2vm.items.move`, sliding the neighbour (moveDisplaced) and settling the
// dragged card (move), exactly like the bar's pill reorder. Release commits
// the whole open-id list via `asks order set` (v2Commit), anchoring a filtered
// move onto the global order. This is the animation-capable successor to an
// earlier proxy+drop-line handle drag — hence the `v2`/`card2`/`asks2` names.
//
// Two anti-jank rules shape the asks mode (both were live bugs — clicking
// around used to re-animate the whole panel):
//  1. IDENTITY-STABLE ROWS. The ListView's DelegateModel model is `v2List`,
//     rebuilt from `askIds` (via onAskIdsChanged) ONLY when the id sequence
//     itself changes (post/dismiss/state moves/reorder). Everything else about
//     a row flows through the `askById` lookup map, so a content change (a
//     note, an urgency bump, the stream echoing our own draft autosave every
//     ~1.5s while typing) updates bindings IN PLACE and never destroys a
//     delegate — the same lookup-map pattern BarWidget uses for pills.
//  2. FIXED PANEL GEOMETRY. contentPreferredHeight is snapshotted ONCE at
//     open (sized to the queue, capped) and never re-bound: the SmartPanel
//     animates geometry changes, so a height that tracked the content made
//     every expand/collapse read as a panel re-open. Rows scroll INSIDE the
//     ListView and expansion changes nothing about the panel's frame.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml.Models
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Widgets
import qs.Services.UI

Item {
  id: root

  property var pluginApi: null

  readonly property var main: pluginApi ? pluginApi.mainInstance : null
  readonly property string mode: main ? main.panelMode : "rename"
  readonly property int wsId: main ? main.pendingRenameId : 0
  // The asks (Deck) mode. Every asks-only gate — geometry, the age timer, the
  // height snapshot, the row model — keys off this.
  readonly property bool asksMode: mode === "asks"

  // SmartPanel contract. Asks is the widest — a triage row needs title, meta
  // and its action cluster side by side without fighting; settings is sized
  // to its full content (SmartPanel clamps to the screen, and the scroll
  // view only kicks in if it can't fit); rename hugs its content.
  readonly property var geometryPlaceholder: panelContainer
  readonly property bool allowAttach: true
  property real contentPreferredWidth: (mode === "settings" ? 580 : asksMode ? 980 : 320) * Style.uiScaleRatio
  // Full natural height of the settings column (header + separator + 3 row gaps +
  // top/bottom margins + the settings content). No artificial cap: SmartPanel
  // clamps to the screen, and the scroll view only kicks in if it can't fit.
  readonly property real _settingsHeight: settingsLoader.implicitHeight + settingsHeader.implicitHeight + chrome.implicitHeight + 1 + Style.marginM * 3 + Style.marginL * 2
  // Asks height: SNAPSHOT, not a binding (anti-jank rule 2 in the header).
  // Sized to the queue at open — chrome + one slot per ask + room for one
  // expanded reply area — capped; growth mid-open scrolls inside instead of
  // resizing the animated panel frame. Overwritten in Component.onCompleted.
  property real asksPanelHeight: 380 * Style.uiScaleRatio
  function computeAsksHeight() {
    var n = Math.max(1, askIds.length);
    return Math.min(760, 108 + n * 46 + 190) * Style.uiScaleRatio;
  }
  property real contentPreferredHeight: (mode === "settings" ? _settingsHeight : asksMode ? asksPanelHeight : renameCol.implicitHeight + Style.marginL * 2)

  // ---- asks state -------------------------------------------------------------
  readonly property var asksRows: (main && main.asksRows) ? main.asksRows : []
  // Identity-stable row model (anti-jank rule 1 in the header): askIds is
  // the bare id sequence in the stream's RESOLVED order and is the row
  // Repeater's model — reassigned only when the sequence changes; askById
  // carries every row's content and is reassigned freely (bindings update
  // delegates in place).
  property var askIds: []
  property var askById: ({})
  onAsksRowsChanged: syncAsks()
  // Keep the v2 ListView's DelegateModel source in step with the id sequence.
  // Fires only when askIds is actually reassigned (a sequence change), so
  // content ticks never churn the visual model (identity-stable, same as v1).
  onAskIdsChanged: v2Sync()
  // "Hide delivered" (default ON): a delivered ask is finished business —
  // asked, answered, and the answer has reached its asker (delivered_at,
  // stamped only by the asker's own MCP collection; contract in
  // ctl/src/lib.rs). Hiding on delivery IS the auto-fade: the row vanishes
  // the moment the agent collects, no timer. Persisted in pluginSettings
  // so the choice survives panel opens and shell restarts.
  property bool hideDelivered: true
  onHideDeliveredChanged: syncAsks()
  // v2 project filter: narrow the global queue to one workspace. -1 = all
  // (v1 never sets it, so v1 is unaffected). Lives in the MEMBERSHIP build
  // (syncAsks) like hideDelivered — changing it is a legitimate id-sequence
  // change (one rebuild), not per-row churn.
  property int filterWs: -1
  onFilterWsChanged: syncAsks()
  // Queue/Board segmented toggle (v2). Board is deferred — the control renders
  // for layout parity but is inert/dimmed until the Board view exists.
  property string deckView: "queue"
  // Project identity colours for the v2 chips + card project tags. Project
  // identity isn't a theme role, so these are fixed distinct hues (the one
  // place v2 steps outside the Noctalia token set), indexed by workspace.
  readonly property var projectPalette: ["#6ea8c9", "#b58ac9", "#d9a04a", "#7bb47a", "#e0855f", "#8fb4c9", "#c99ad9", "#89b48f"]
  function projectColor(ws) {
    if (ws === null || ws === undefined)
      return Color.mOnSurfaceVariant;
    return projectPalette[Math.abs(ws) % projectPalette.length];
  }
  function loadHideDelivered() {
    if (pluginApi && pluginApi.pluginSettings)
      hideDelivered = pluginApi.pluginSettings.asksHideDelivered !== false;
  }
  onPluginApiChanged: {
    loadHideDelivered();
    loadInject();
    loadRetrigger();
  }
  function setHideDelivered(on) {
    hideDelivered = on;
    if (pluginApi && pluginApi.pluginSettings) {
      pluginApi.pluginSettings.asksHideDelivered = on;
      pluginApi.saveSettings();
    }
  }

  // ---- "Inject" checkbox: gate the per-turn UserPromptSubmit nudge -----------
  // On: bsctl's UserPromptSubmit hook (`asks inject`) injects the Deck reminder
  // + presence + open-count into every agent turn. pluginSettings is the
  // PERSISTENT truth; bsctl's runtime flag (what the hook reads) is ephemeral —
  // cleared on reboot — so we PUSH the preference to it on load and on every
  // toggle. injectProc is separate from askProc so the sync never clobbers an
  // in-flight asks action (same reasoning as wakeProc).
  property bool injectOn: false
  function pushInject(on) {
    injectProc.command = [Quickshell.env("HOME") + "/.local/bin/bsctl", "asks", "inject", on ? "on" : "off"];
    injectProc.running = true;
  }
  function loadInject() {
    if (pluginApi && pluginApi.pluginSettings)
      injectOn = pluginApi.pluginSettings.asksInject === true;
    pushInject(injectOn);
  }
  function setInject(on) {
    injectOn = on;
    if (pluginApi && pluginApi.pluginSettings) {
      pluginApi.pluginSettings.asksInject = on;
      pluginApi.saveSettings();
    }
    pushInject(on);
  }

  // ---- "Background retrigger" checkbox: Stop-hook backstop -------------------
  // On: when a turn ends with a question while the human isn't looking at this
  // session, bsctl's Stop hook re-prompts the agent to post it to the Deck.
  // Same persistence model as Inject (pluginSettings truth, pushed to bsctl's
  // ephemeral flag on load and on toggle).
  property bool retriggerOn: false
  function pushRetrigger(on) {
    retriggerProc.command = [Quickshell.env("HOME") + "/.local/bin/bsctl", "asks", "retrigger", on ? "on" : "off"];
    retriggerProc.running = true;
  }
  function loadRetrigger() {
    if (pluginApi && pluginApi.pluginSettings)
      retriggerOn = pluginApi.pluginSettings.asksRetrigger === true;
    pushRetrigger(retriggerOn);
  }
  function setRetrigger(on) {
    retriggerOn = on;
    if (pluginApi && pluginApi.pluginSettings) {
      pluginApi.pluginSettings.asksRetrigger = on;
      pluginApi.saveSettings();
    }
    pushRetrigger(on);
  }
  function sameIdList(a, b) {
    if (!a || !b || a.length !== b.length)
      return false;
    for (var i = 0; i < a.length; i++)
      if (a[i] !== b[i])
        return false;
    return true;
  }
  function syncAsks() {
    var rows = asksRows;
    var ids = [];
    var by = {};
    for (var i = 0; i < rows.length; i++) {
      // The filter lives in the MEMBERSHIP build on purpose: hiding a
      // freshly-delivered row is a legitimate sequence change (one
      // rebuild), while a delivered_at landing with the filter OFF is
      // content-only — ids unchanged, no delegate churn.
      if (hideDelivered && rows[i].delivered_at)
        continue;
      if (filterWs >= 0 && rows[i].ws !== filterWs)
        continue;
      ids.push(rows[i].id);
      by[String(rows[i].id)] = rows[i];
    }
    // byId first: a delegate whose id just vanished renders one harmless
    // empty frame off the ({}) fallback before the ids reassignment (same
    // JS turn) destroys it — never a dangling lookup.
    askById = by;
    if (!sameIdList(ids, askIds))
      askIds = ids;
  }
  // Single-expansion: at most one row's reply area is open (triage is one
  // ask at a time, and collapsing everything at drag start is then trivial).
  property int expandedId: -1
  // The expanded row's reply text, held OUTSIDE the delegates: an
  // id-sequence change (another agent posting, a dismiss, a state move)
  // still rebuilds the delegates mid-typing, so the in-progress text must
  // survive on the panel. Snapshotted from the ask's saved reply at
  // expansion, tracked per keystroke, restored by the input's
  // Component.onCompleted after any such rebuild. (Content-only changes —
  // including the echo of our own autosave — no longer rebuild anything.)
  property string draftText: ""
  // The last text actually written to the store for the expanded ask —
  // every persist path dedupes against it, so the stream's echo of our own
  // write (which rebuilds the delegates) never triggers another write.
  property string lastPersisted: ""

  // Auto-save: a draft is persisted (`asks reply`) on a short typing
  // debounce, on blur, on collapse and on close — never on a button. All
  // paths funnel through persistDraft(), which skips unchanged text.
  Timer {
    id: draftTimer
    interval: 1500
    onTriggered: root.persistDraft()
  }
  function persistDraft() {
    draftTimer.stop();
    if (expandedId >= 0 && draftText !== lastPersisted) {
      replyAsk(expandedId, draftText);
      lastPersisted = draftText;
    }
  }
  // Done / an option click / dismiss supersede the draft: stop the pending
  // debounce and mark the current text as settled so no later persist path
  // resurrects it over the final answer.
  function cancelPendingDraft() {
    draftTimer.stop();
    lastPersisted = draftText;
  }
  function collapseExpanded() {
    persistDraft();
    expandedId = -1;
  }
  // Row-click toggle. Expanding snapshots the saved reply into the
  // panel-held draft (the input restores from it if delegates rebuild);
  // switching rows persists the old row's draft first.
  function toggleExpand(row) {
    if (expandedId === row.askId) {
      collapseExpanded();
      return;
    }
    persistDraft();
    draftText = row.ask.answer || "";
    lastPersisted = draftText;
    expandedId = row.askId;
  }
  // Age text ticks while the panel is up (rows only re-render on stream
  // changes; age would otherwise freeze at open time).
  property real nowS: Date.now() / 1000
  Timer {
    interval: 30000
    running: root.asksMode
    repeat: true
    onTriggered: root.nowS = Date.now() / 1000
  }


  function openIds() {
    var ids = [];
    for (var i = 0; i < asksRows.length; i++)
      if (asksRows[i].state === "open")
        ids.push(asksRows[i].id);
    return ids;
  }
  // The open ids ACTUALLY ON SCREEN, in display order — askIds (which already
  // applies the hideDelivered + filterWs membership) narrowed to open rows.
  // Under a project filter this is a subset of openIds(); with no filter the
  // two match. Drag slot-count and the reorder anchor both use this so a
  // filtered reorder maps correctly onto the global order.
  function visibleOpenIds() {
    var ids = [];
    for (var i = 0; i < askIds.length; i++) {
      var r = askById[String(askIds[i])];
      if (r && r.state === "open")
        ids.push(askIds[i]);
    }
    return ids;
  }

  // ---- v2 (bar-style) drag-reflow --------------------------------------------
  // v2 animates reorders like the bar: a ListView + DelegateModel whose items
  // are MOVED (not recreated) mid-drag, so neighbours slide (moveDisplaced) and
  // the dragged card settles (move). v2List is that model — rebuilt from askIds
  // only on a sequence change (via onAskIdsChanged) and frozen while a reorder
  // is in flight so items.move isn't fought by a model rebuild.
  property var v2List: []
  property bool v2Reordering: false
  property int v2DragId: -1
  property int v2DragFrom: -1
  property int v2DragTo: -1
  property var v2OrigOpen: []
  function v2Sync() {
    if (v2Reordering)
      return;
    var lst = [];
    for (var i = 0; i < askIds.length; i++)
      lst.push({
        "id": askIds[i]
      });
    v2List = lst;
  }
  // Settled center-y (content coords) of the row at visible index idx, summed
  // from row HEIGHTS (which don't animate) so a swap never reacts to an
  // in-flight slide — the bar's slotCenterX, vertical.
  function v2SlotCenterY(idx) {
    var y = 0;
    for (var i = 0; i < idx; i++) {
      var it = rows2List.itemAtIndex(i);
      y += (it ? it.height : 0) + rows2List.spacing;
    }
    var self = rows2List.itemAtIndex(idx);
    return y + (self ? self.height : 0) / 2;
  }
  function v2Begin(id, visIdx) {
    collapseExpanded(); // uniform row heights for the duration of the reflow
    v2Reordering = true;
    v2DragId = id;
    v2DragFrom = visIdx;
    v2DragTo = visIdx;
    v2OrigOpen = visibleOpenIds();
  }
  function v2DragStep(contentY) {
    if (v2DragId < 0)
      return;
    var openN = v2OrigOpen.length; // open rows lead; never cross into the answered tail
    var d = v2DragTo;
    // Swap with a neighbour once the cursor passes its settled centre; after a
    // swap the neighbour is a full row away, so the reverse threshold has
    // built-in hysteresis (no oscillation) — same as the bar.
    if (d + 1 < openN && contentY > v2SlotCenterY(d + 1)) {
      v2vm.items.move(d, d + 1);
      v2DragTo = d + 1;
    } else if (d - 1 >= 0 && contentY < v2SlotCenterY(d - 1)) {
      v2vm.items.move(d, d - 1);
      v2DragTo = d - 1;
    }
  }
  function v2Commit() {
    if (!v2Reordering)
      return;
    var from = v2DragFrom;
    var to = v2DragTo;
    v2Reordering = false;
    v2DragId = -1;
    // Translate the visible move onto the GLOBAL open order by anchoring: the
    // dragged item is spliced directly next to the visible neighbour it landed
    // beside — beneath the item now above it (moving down) or above the item
    // now below it (moving up). Hidden (other-project) items keep their places,
    // so this is correct under a project filter and reduces to a plain splice
    // when unfiltered.
    if (to !== from && from >= 0 && to >= 0 && from < v2OrigOpen.length && to < v2OrigOpen.length) {
      var draggedId = v2OrigOpen[from];
      var newVis = v2OrigOpen.slice();
      newVis.splice(from, 1);
      newVis.splice(to, 0, draggedId);
      var movingDown = to > from;
      var anchor = movingDown ? newVis[to - 1] : newVis[to + 1];
      var global = openIds();
      var gi = global.indexOf(draggedId);
      if (gi >= 0)
        global.splice(gi, 1);
      var ai = global.indexOf(anchor);
      if (ai < 0)
        global.push(draggedId);
      else
        global.splice(movingDown ? ai + 1 : ai, 0, draggedId);
      bsctl(["asks", "order", "set"].concat(global.map(String)));
      // Leave the DelegateModel as items.move left it (already the new order);
      // the stream echo rebuilds v2List to the same order — no flash, no revert.
    }
  }
  function v2Cancel() {
    v2Reordering = false;
    v2DragId = -1;
    v2Sync(); // snap the visual back to the model order
  }

  // Presentation helpers for ask rows. (Cfg has the pill-side helpers; the
  // panel has no screen to instantiate a Cfg against, so these small
  // functions live here.)
  // The row DOT is the LIVE signal, not urgency: green = an agent's ask
  // call is parked on this row right now, holding its turn open (the
  // stream's `blocking`, pid-sanitized in bsctl so a crashed server can't
  // lie); grey = nobody waits live. mTertiary is the green accent under
  // the current scheme (same verification as the old urgency-medium dot).
  function dotColor(blocking) {
    return blocking ? Color.mTertiary : Qt.alpha(Color.mOnSurface, 0.35);
  }
  // Urgency is TEXT in the meta line (the user's design): high red,
  // med blue (mPrimary — the scheme's blue accent, per the focused-pill
  // swatch ground truth), low plain foreground.
  function urgencyLabel(u) {
    return u === "medium" ? "med" : u || "low";
  }
  function urgencyTextColor(u) {
    return u === "high" ? Color.mError : u === "medium" ? Color.mPrimary : Color.mOnSurfaceVariant;
  }
  function fmtAge(created) {
    if (!created)
      return "";
    var s = Math.max(0, Math.round(nowS - created));
    if (s < 60)
      return s + "s";
    if (s < 3600)
      return Math.round(s / 60) + "m";
    return Math.floor(s / 3600) + "h";
  }
  function askMeta(r) {
    // Order per the user's triage grammar: id first (the handle every verb
    // takes), then who, then where (ws number + name), then when/how-long.
    // Urgency is NOT here — it renders as its own colored segment beside
    // this text (askMetaRow); blocking gets a textual echo so the dot's
    // green has words.
    var parts = ["#" + r.id];
    if (r.blocking)
      parts.push("blocking — agent waiting");
    if (r.kind)
      parts.push(r.kind);
    if (r.ws !== null && r.ws !== undefined)
      parts.push("ws " + r.ws + (r.wsName ? " — " + r.wsName : ""));
    parts.push(fmtAge(r.created));
    if (r.estimate_min)
      parts.push("~" + r.estimate_min + "m of you");
    if (r.state === "answered")
      // delivered_at is the honest discriminator: "awaiting pickup" only
      // while the asker really hasn't collected (contract in ctl/src/lib.rs)
      parts.push(r.delivered_at ? "delivered ✓" : "answered — awaiting pickup");
    else if (r.answer)
      parts.push("draft: “" + r.answer + "”"); // reply saved, not yet completed
    if (r.note)
      parts.push("“" + r.note + "”");
    return parts.join("  ·  ");
  }

  // ---- v2 skin helpers --------------------------------------------------------
  // Header counts read the WHOLE queue (global), not the filtered view. Called
  // from bindings that also touch asksRows.length so they re-evaluate on any
  // stream change.
  function openAsksCount() {
    var n = 0;
    for (var i = 0; i < asksRows.length; i++)
      if (asksRows[i].state === "open")
        n++;
    return n;
  }
  function blockingAsksCount() {
    var n = 0;
    for (var i = 0; i < asksRows.length; i++)
      if (asksRows[i].blocking === true && asksRows[i].state === "open")
        n++;
    return n;
  }
  // Distinct workspaces with a visible (non-delivered) ask — the filter-chip
  // set. Independent of the current filter so every project stays reachable.
  function projectChips() {
    var seen = {};
    var out = [];
    for (var i = 0; i < asksRows.length; i++) {
      var r = asksRows[i];
      if (hideDelivered && r.delivered_at)
        continue;
      if (r.ws === null || r.ws === undefined)
        continue;
      var k = String(r.ws);
      if (!seen[k]) {
        seen[k] = {
          ws: r.ws,
          name: r.wsName || "",
          color: projectColor(r.ws),
          count: 0
        };
        out.push(seen[k]);
      }
      seen[k].count++;
    }
    out.sort(function (a, b) {
      return a.ws - b.ws;
    });
    return out;
  }

  // ---- asks actions (direct argv: no shell, no quoting) -----------------------
  function bsctl(args) {
    askProc.command = [Quickshell.env("HOME") + "/.local/bin/bsctl"].concat(args);
    askProc.running = true;
  }
  // The asker's live status (waiting/thinking/tooling), joined from the bar's
  // stream via the singleton; "" when unknown. Drives the answer button's
  // label and the wake decision.
  function sessionStatus(session) {
    if (!main || !main.statusBySid || !session)
      return "";
    return main.statusBySid[String(session)] || "";
  }
  // Will answering this ask poke the asker's terminal? Only when it is parked
  // IDLE (waiting) and NOT blocking — a blocking asker's `ask` RPC returns the
  // answer directly (no prompt to type into), and a busy asker collects it at
  // its next turn. Everything else just enqueues. This is the Trigger/Enqueue
  // discriminator (contract in ctl/src/lib.rs, "DELIVERY vs WAKE").
  function willWake(ask) {
    return !!ask && ask.blocking !== true && sessionStatus(ask.session) === "waiting";
  }
  // The answer path shared by the option buttons, the answer button and the
  // input's Enter. Always stores the answer (the queue is the source of
  // truth); when the asker is idle, also fires `asks wake` — a TRIGGER, not a
  // delivery: it types "call get_ask N" so the asker fetches the answer
  // itself. The two are separate Processes fired back to back; ordering at the
  // agent is causal (get_ask runs a whole turn later, long after the ms-scale
  // answer write lands), so no explicit sequencing is needed. `asks wake`
  // self-guards (blocking → no-op, busy/socket-less → the harness collects
  // later), so a status race between label and click degrades to an enqueue.
  function submitAnswer(id, text) {
    cancelPendingDraft(); // the answer supersedes any in-flight draft write
    var wake = willWake(root.askById[String(id)]);
    if (text.length > 0)
      bsctl(["asks", "answer", String(id), text]);
    else
      bsctl(["asks", "complete", String(id)]);
    if (wake) {
      wakeProc.command = [Quickshell.env("HOME") + "/.local/bin/bsctl", "asks", "wake", String(id)];
      wakeProc.running = true;
    }
    root.expandedId = -1;
  }
  // The auto-save write: update the reply WITHOUT completing — a draft
  // while "still working on it". The store keeps state open, so a blocked
  // asker stays parked; agents peeking via get_ask see the draft. Empty
  // text clears.
  function replyAsk(id, text) {
    bsctl(["asks", "reply", String(id), text]);
  }
  function reopenAsk(id) {
    bsctl(["asks", "reopen", String(id)]);
  }
  function noteAsk(id, text) {
    bsctl(["asks", "note", String(id), text]);
  }
  function dismissAsk(id) {
    if (id === root.expandedId) {
      cancelPendingDraft(); // a reply to a dismissed ask would be refused anyway
      root.expandedId = -1;
    }
    bsctl(["asks", "dismiss", String(id)]);
  }
  // Jump lands on the asking session's exact WINDOW (ws focus --session:
  // window dispatch when the session record knows its win, workspace
  // fallback otherwise — never less than the old --ws-id form).
  function jumpToAsk(session) {
    bsctl(["ws", "focus", "--session", session]);
    close();
  }

  function renameSubmit() {
    // Positional args keep the name opaque to the shell; bsctl escapes it and
    // `name set` resets to the number when the name is empty.
    renameProc.command = ["sh", "-c", "$HOME/.local/bin/bsctl ws name set --ws-id \"$1\" --name \"$2\"", "sh", String(wsId), renameInput.text];
    renameProc.running = true;
    close();
  }

  function settingsApply() {
    if (settingsLoader.item && settingsLoader.item.saveSettings)
      settingsLoader.item.saveSettings();
    close();
  }

  function close() {
    persistDraft(); // the panel's own close paths flush the draft first
    if (pluginApi)
      pluginApi.closePanel(pluginApi.panelOpenScreen);
  }

  // Best-effort flush when the panel is torn down from OUTSIDE (the
  // HYPER-key/badge toggle closes via Main.qml, not close()). The Process
  // spawn races the destruction, so the debounce remains the primary
  // guarantee — this catches keystrokes younger than one debounce.
  Component.onDestruction: persistDraft()

  Process {
    id: renameProc
  }
  Process {
    id: askProc
  }
  // Separate from askProc so the answer write and the wake nudge never clobber
  // each other's command mid-run (they fire back to back in submitAnswer).
  Process {
    id: wakeProc
  }
  // Separate from askProc (see wakeProc): the inject-flag sync fires on panel
  // load and on checkbox toggle, independent of any asks action in flight.
  Process {
    id: injectProc
  }
  // Likewise for the background-retrigger flag sync.
  Process {
    id: retriggerProc
  }

  // Config/icon layer for the v2 skin's harness BotIcons. The bar builds its
  // own Cfg in BarWidget and passes it down; the panel has no such parent, so
  // it makes its own against the open screen (Cfg is a plain QtObject — cheap,
  // non-visual). Reused by every v2 row's BotIcon.
  Cfg {
    id: deckCfg
    pluginApi: root.pluginApi
    screen: root.pluginApi ? root.pluginApi.panelOpenScreen : null
  }

  Item {
    id: panelContainer
    anchors.fill: parent

    // ---- rename ----
    ColumnLayout {
      id: renameCol
      visible: root.mode === "rename"
      anchors.centerIn: parent
      width: parent.width - Style.marginL * 2
      spacing: Style.marginM

      NText {
        text: "Rename workspace"
        pointSize: Style.fontSizeL
        font.weight: Style.fontWeightBold
        color: Color.mOnSurface
        Layout.fillWidth: true
      }

      NTextInput {
        id: renameInput
        Layout.fillWidth: true
        placeholderText: "Name (empty resets to the number)"
        onAccepted: root.renameSubmit()
      }

      RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Style.marginS
        spacing: Style.marginM

        Item {
          Layout.fillWidth: true
        }
        NButton {
          text: "Cancel"
          backgroundColor: Color.mSurfaceVariant
          textColor: Color.mOnSurfaceVariant
          outlined: false
          onClicked: root.close()
        }
        NButton {
          text: "Rename"
          backgroundColor: Color.mPrimary
          textColor: Color.mOnPrimary
          onClicked: root.renameSubmit()
        }
      }
    }


    // ---- asks (the Deck) ----
    // The asks-queue triage surface. Rows are a ListView + DelegateModel so a
    // reorder animates (neighbours slide, the dragged card settles); the grip
    // drives it via v2vm.items.move and commits with `asks order set`. The
    // internal `asks2`/`v2`/`card2` names are historical (this replaced an
    // earlier skin) — kept to avoid a churny rename.
    ColumnLayout {
      id: asks2Col
      visible: root.mode === "asks"
      anchors.fill: parent
      anchors.margins: Style.marginL
      spacing: Style.marginM

      // header: title + global counts (left), pill toggles + segmented
      // Queue/Board + close (right)
      RowLayout {
        Layout.fillWidth: true
        spacing: Style.marginM

        NText {
          text: "The Deck"
          pointSize: Style.fontSizeL
          font.weight: Style.fontWeightBold
          color: Color.mOnSurface
        }
        NText {
          text: (root.asksRows.length >= 0) ? (root.openAsksCount() + " open  ·  " + root.blockingAsksCount() + " blocking") : ""
          font.family: "Noto Sans Mono"
          pointSize: Style.fontSizeXS
          color: Color.mOnSurfaceVariant
        }

        Item {
          Layout.fillWidth: true
        }

        // automation toggles as pill buttons (check-in-pill)
        RowLayout {
          spacing: Style.marginXS
          Repeater {
            model: [
              {
                label: "Inject",
                on: root.injectOn,
                kind: "inject"
              },
              {
                label: "Auto-retrigger",
                on: root.retriggerOn,
                kind: "retrigger"
              },
              {
                label: "Hide delivered",
                on: root.hideDelivered,
                kind: "hide"
              }
            ]
            delegate: Rectangle {
              required property var modelData
              radius: Style.radiusXS
              implicitHeight: pillRow.implicitHeight + Style.marginXS * 2
              implicitWidth: pillRow.implicitWidth + Style.marginS * 2
              color: modelData.on ? Qt.alpha(Color.mPrimary, 0.14) : Qt.alpha(Color.mOnSurface, 0.04)
              border.width: 1
              border.color: modelData.on ? Qt.alpha(Color.mPrimary, 0.45) : Qt.alpha(Color.mOnSurface, 0.10)
              RowLayout {
                id: pillRow
                anchors.centerIn: parent
                spacing: Style.marginXS
                Rectangle {
                  implicitWidth: 14
                  implicitHeight: 14
                  radius: 4
                  color: modelData.on ? Color.mPrimary : "transparent"
                  border.width: 1
                  border.color: modelData.on ? Color.mPrimary : Qt.alpha(Color.mOnSurface, 0.30)
                  NIcon {
                    anchors.centerIn: parent
                    visible: modelData.on
                    icon: "check"
                    pointSize: Style.fontSizeXS
                    color: Color.mOnPrimary
                  }
                }
                NText {
                  text: modelData.label
                  pointSize: Style.fontSizeS
                  font.weight: Style.fontWeightBold
                  color: modelData.on ? Color.mPrimary : Color.mOnSurfaceVariant
                }
              }
              MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                  if (modelData.kind === "inject")
                    root.setInject(!root.injectOn);
                  else if (modelData.kind === "retrigger")
                    root.setRetrigger(!root.retriggerOn);
                  else
                    root.setHideDelivered(!root.hideDelivered);
                }
              }
            }
          }
        }

        // segmented Queue / Board (Board deferred → dimmed + inert)
        Rectangle {
          radius: Style.radiusXS
          color: Qt.alpha(Color.mOnSurface, 0.06)
          border.width: 1
          border.color: Qt.alpha(Color.mOnSurface, 0.10)
          implicitHeight: segRow.implicitHeight + 6
          implicitWidth: segRow.implicitWidth + 6
          RowLayout {
            id: segRow
            anchors.centerIn: parent
            spacing: 2
            Rectangle {
              radius: Style.radiusXS
              color: root.deckView === "queue" ? Color.mPrimary : "transparent"
              implicitHeight: qTxt.implicitHeight + Style.marginXS * 2
              implicitWidth: qTxt.implicitWidth + Style.marginM
              NText {
                id: qTxt
                anchors.centerIn: parent
                text: "☰ Queue"
                pointSize: Style.fontSizeS
                font.weight: Style.fontWeightBold
                color: root.deckView === "queue" ? Color.mOnPrimary : Color.mOnSurfaceVariant
              }
              MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.deckView = "queue"
              }
            }
            Rectangle {
              radius: Style.radiusXS
              color: "transparent"
              opacity: 0.4
              implicitHeight: bTxt.implicitHeight + Style.marginXS * 2
              implicitWidth: bTxt.implicitWidth + Style.marginM
              NText {
                id: bTxt
                anchors.centerIn: parent
                text: "▤ Board"
                pointSize: Style.fontSizeS
                font.weight: Style.fontWeightBold
                color: Color.mOnSurfaceVariant
              }
            }
          }
        }

        NIconButton {
          icon: "close"
          customRadius: Style.radiusXS
          onClicked: root.close()
        }
      }

      // project filter chips: narrow the global queue to one workspace
      Flow {
        Layout.fillWidth: true
        spacing: Style.marginXS
        Rectangle {
          id: allChip
          readonly property bool active: root.filterWs < 0
          radius: Style.radiusXS
          implicitHeight: allChipTxt.implicitHeight + Style.marginXS * 2
          implicitWidth: allChipTxt.implicitWidth + Style.marginM * 2
          color: allChip.active ? Qt.alpha(Color.mPrimary, 0.16) : Qt.alpha(Color.mOnSurface, 0.04)
          border.width: 1
          border.color: allChip.active ? Qt.alpha(Color.mPrimary, 0.45) : Qt.alpha(Color.mOnSurface, 0.10)
          NText {
            id: allChipTxt
            anchors.centerIn: parent
            text: "All projects"
            pointSize: Style.fontSizeXS
            color: allChip.active ? Color.mPrimary : Color.mOnSurfaceVariant
          }
          MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: root.filterWs = -1
          }
        }
        Repeater {
          model: (root.asksRows.length >= 0) ? root.projectChips() : []
          delegate: Rectangle {
            id: pchip
            required property var modelData
            readonly property bool active: root.filterWs === modelData.ws
            radius: Style.radiusXS
            implicitHeight: pchipRow.implicitHeight + Style.marginXS * 2
            implicitWidth: pchipRow.implicitWidth + Style.marginM * 2
            color: pchip.active ? Qt.alpha(Color.mPrimary, 0.16) : Qt.alpha(Color.mOnSurface, 0.04)
            border.width: 1
            border.color: pchip.active ? Qt.alpha(Color.mPrimary, 0.45) : Qt.alpha(Color.mOnSurface, 0.10)
            RowLayout {
              id: pchipRow
              anchors.centerIn: parent
              spacing: Style.marginXS
              Rectangle {
                implicitWidth: 8
                implicitHeight: 8
                radius: 4
                color: pchip.modelData.color
              }
              NText {
                text: (pchip.modelData.name && pchip.modelData.name.length > 0) ? pchip.modelData.name : ("ws " + pchip.modelData.ws)
                pointSize: Style.fontSizeXS
                font.weight: pchip.active ? Style.fontWeightBold : Style.fontWeightRegular
                color: pchip.active ? Color.mOnSurface : Color.mOnSurfaceVariant
              }
              NText {
                text: String(pchip.modelData.count)
                font.family: "Noto Sans Mono"
                pointSize: Style.fontSizeXS
                color: Color.mOnSurfaceVariant
                opacity: 0.7
              }
            }
            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: root.filterWs = (root.filterWs === pchip.modelData.ws ? -1 : pchip.modelData.ws)
            }
          }
        }
      }

      Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 1
        color: Color.mOutline
      }

      Item {
        Layout.fillWidth: true
        Layout.fillHeight: true

        NText {
          anchors.centerIn: parent
          visible: root.askIds.length === 0
          text: root.filterWs >= 0 ? "no asks in this project" : "the Deck is clear"
          color: Color.mOnSurfaceVariant
        }

        // ListView + DelegateModel so a reorder ANIMATES (neighbours slide via
        // moveDisplaced, the moved card settles via move) — the bar's mechanism.
        // The grip drag calls v2vm.items.move to reflow live; v2Commit persists.
        ListView {
          id: rows2List
          anchors.fill: parent
          clip: true
          spacing: Style.marginM
          cacheBuffer: 100000 // keep every delegate realised so a reorder never recycles one
          boundsBehavior: Flickable.StopAtBounds
          ScrollBar.vertical: ScrollBar {}
          model: DelegateModel {
            id: v2vm
            model: root.mode === "asks" ? root.v2List : []
            delegate: v2CardComponent
          }
          move: Transition {
            NumberAnimation {
              properties: "y"
              duration: Style.animationFast
              easing.type: Easing.OutQuad
            }
          }
          moveDisplaced: Transition {
            NumberAnimation {
              properties: "y"
              duration: Style.animationFast
              easing.type: Easing.OutQuad
            }
          }
        }
      }

      Component {
        id: v2CardComponent

        Rectangle {
          id: card2
          required property var modelData
          required property int index
          readonly property int askId: modelData.id
          readonly property var ask: root.askById[String(modelData.id)] || ({})
          readonly property bool replying: root.expandedId === askId
          readonly property bool open: ask.state === "open"
          readonly property bool isNotify: ask.type === "notify"
          readonly property bool dragging: root.v2DragId === askId

          width: ListView.view ? ListView.view.width : 0
          height: card2Col.implicitHeight + Style.marginL * 2
          z: dragging ? 1000 : 0
          scale: dragging ? 0.98 : 1.0
          Behavior on scale {
            NumberAnimation {
              duration: Style.animationFaster
            }
          }
          radius: Style.radiusS
          color: card2.replying ? Qt.alpha(Color.mSurfaceVariant, 0.45) : (dragging ? Qt.alpha(Color.mSurfaceVariant, 0.6) : Qt.alpha(Color.mOnSurface, 0.04))
          border.width: 1
          border.color: (card2.replying || dragging) ? Qt.alpha(Color.mPrimary, 0.45) : Qt.alpha(Color.mOnSurface, 0.08)

              // urgency accent — a flush 3px left edge (CSS border-left:3px)
              Rectangle {
                anchors.left: parent.left
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                anchors.topMargin: Style.marginXXS
                anchors.bottomMargin: Style.marginXXS
                width: Style.borderL
                radius: Style.borderL / 2
                color: root.urgencyTextColor(card2.ask.urgency)
                opacity: card2.open ? 1.0 : 0.5
              }

              ColumnLayout {
                id: card2Col
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.leftMargin: Style.marginL
                anchors.rightMargin: Style.marginM
                anchors.topMargin: Style.marginL
                spacing: Style.marginM

                // clickable header (title + type glyph + right-side actions)
                Item {
                  Layout.fillWidth: true
                  implicitHeight: head2.implicitHeight
                  Rectangle {
                    anchors.fill: parent
                    anchors.margins: -Style.marginXS
                    radius: Style.radiusXS
                    color: card2Area.containsMouse ? Qt.alpha(Color.mOnSurface, 0.05) : "transparent"
                  }
                  MouseArea {
                    id: card2Area
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: root.toggleExpand(card2)
                  }
                  RowLayout {
                    id: head2
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Style.marginM

                    // drag handle — reorders the queue via `asks order set`.
                    // Works filtered too: v2Commit anchors the move onto the
                    // global order (see v2Commit). Open rows only.
                    Item {
                      // slot reserved on every row so glyphs/titles align
                      Layout.preferredWidth: 14
                      Layout.preferredHeight: 24
                      NText {
                        anchors.centerIn: parent
                        visible: card2.open
                        text: "⠿"
                        pointSize: Style.fontSizeM
                        color: (h2Handle.containsMouse || card2.dragging) ? Color.mOnSurface : Qt.alpha(Color.mOnSurface, 0.35)
                      }
                      MouseArea {
                        id: h2Handle
                        anchors.fill: parent
                        enabled: card2.open
                        hoverEnabled: true
                        preventStealing: true
                        cursorShape: card2.dragging ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                        onPressed: mouse => root.v2Begin(card2.askId, card2.DelegateModel.itemsIndex)
                        onPositionChanged: mouse => {
                          if (pressed) {
                            var p = h2Handle.mapToItem(rows2List.contentItem, mouse.x, mouse.y);
                            root.v2DragStep(p.y);
                          }
                        }
                        onReleased: root.v2Commit()
                        onCanceled: root.v2Cancel()
                      }
                    }

                    Rectangle {
                      implicitWidth: 24
                      implicitHeight: 24
                      radius: 12
                      color: "transparent"
                      border.width: 1
                      border.color: card2.isNotify ? Color.mSecondary : root.urgencyTextColor(card2.ask.urgency)
                      opacity: card2.open ? 1.0 : 0.55
                      NIcon {
                        anchors.centerIn: parent
                        icon: card2.isNotify ? "info-circle" : card2.ask.type === "review" ? "eye" : "question-mark"
                        pointSize: Style.fontSizeS
                        color: card2.isNotify ? Color.mSecondary : root.urgencyTextColor(card2.ask.urgency)
                      }
                    }
                    NText {
                      text: card2.ask.title || ""
                      font.weight: Style.fontWeightBold
                      color: Color.mOnSurface
                      opacity: card2.open ? 1.0 : 0.65
                      Layout.fillWidth: true
                    }
                    NButton {
                      visible: (card2.ask.session || "").length > 0
                      text: "Jump ↵"
                      outlined: true
                      buttonRadius: Style.radiusXS
                      onClicked: root.jumpToAsk(card2.ask.session)
                    }
                    NButton {
                      visible: card2.ask.state === "answered"
                      text: "Reopen"
                      outlined: true
                      buttonRadius: Style.radiusXS
                      onClicked: root.reopenAsk(card2.askId)
                    }
                    NText {
                      text: "▾"
                      pointSize: Style.fontSizeS
                      color: Color.mOnSurfaceVariant
                      rotation: card2.replying ? 180 : 0
                      Behavior on rotation {
                        NumberAnimation {
                          duration: 130
                        }
                      }
                    }
                    NIconButton {
                      icon: "close"
                      customRadius: Style.radiusXS
                      onClicked: root.dismissAsk(card2.askId)
                    }
                  }
                }

                // metadata chips: project · harness (BotIcon) · id · age · blocking
                Flow {
                  Layout.fillWidth: true
                  spacing: Style.marginS

                  Rectangle {
                    visible: card2.ask.ws !== null && card2.ask.ws !== undefined
                    radius: Style.radiusXS
                    implicitHeight: projTxt.implicitHeight + Style.marginXXS * 2
                    implicitWidth: projTxt.implicitWidth + Style.marginXS * 2
                    color: Qt.alpha(root.projectColor(card2.ask.ws), 0.16)
                    NText {
                      id: projTxt
                      anchors.centerIn: parent
                      text: card2.ask.ws + (card2.ask.wsName ? " · " + card2.ask.wsName : "")
                      font.family: "Noto Sans Mono"
                      pointSize: Style.fontSizeXS
                      color: root.projectColor(card2.ask.ws)
                    }
                  }

                  RowLayout {
                    visible: (card2.ask.kind || "").length > 0 || (card2.ask.session || "").length > 0
                    spacing: Style.marginXXS
                    BotIcon {
                      cfg: deckCfg
                      kind: card2.ask.kind || "claude"
                      status: root.sessionStatus(card2.ask.session)
                      sizeScale: 0.85
                    }
                    NText {
                      text: card2.ask.kind || "claude"
                      font.family: "Noto Sans Mono"
                      pointSize: Style.fontSizeXS
                      color: Color.mOnSurfaceVariant
                    }
                  }

                  NText {
                    text: "#" + card2.ask.id
                    font.family: "Noto Sans Mono"
                    pointSize: Style.fontSizeXS
                    color: Color.mOnSurfaceVariant
                    opacity: 0.8
                  }
                  NText {
                    text: root.fmtAge(card2.ask.created)
                    font.family: "Noto Sans Mono"
                    pointSize: Style.fontSizeXS
                    color: Color.mOnSurfaceVariant
                    opacity: 0.8
                  }
                  Rectangle {
                    visible: card2.ask.blocking === true
                    radius: Style.radiusXS
                    implicitHeight: blkTxt.implicitHeight + Style.marginXXS * 2
                    implicitWidth: blkTxt.implicitWidth + Style.marginS * 2
                    color: Qt.alpha(Color.mError, 0.13)
                    border.width: 1
                    border.color: Qt.alpha(Color.mError, 0.35)
                    NText {
                      id: blkTxt
                      anchors.centerIn: parent
                      text: "▹ BLOCKING"
                      font.family: "Noto Sans Mono"
                      font.weight: Style.fontWeightBold
                      pointSize: Style.fontSizeXS
                      color: Color.mError
                    }
                  }
                }

                // expanded detail (only for the one expanded row)
                ColumnLayout {
                  visible: card2.replying
                  Layout.fillWidth: true
                  Layout.topMargin: Style.marginXS
                  spacing: Style.marginM

                  // terminal transcript: header bar · divider · body
                  Rectangle {
                    visible: (card2.ask.body || "").length > 0
                    Layout.fillWidth: true
                    radius: Style.radiusXS
                    color: Qt.rgba(0, 0, 0, 0.30)
                    border.width: 1
                    border.color: Qt.alpha(Color.mPrimary, 0.15)
                    clip: true
                    implicitHeight: termBox.implicitHeight

                    ColumnLayout {
                      id: termBox
                      anchors.left: parent.left
                      anchors.right: parent.right
                      anchors.top: parent.top
                      spacing: 0

                      // header bar
                      Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: termHdr.implicitHeight + Style.marginS * 2
                        color: Qt.alpha(Color.mOnSurface, 0.03)
                        RowLayout {
                          id: termHdr
                          anchors.left: parent.left
                          anchors.right: parent.right
                          anchors.verticalCenter: parent.verticalCenter
                          anchors.leftMargin: Style.marginM
                          anchors.rightMargin: Style.marginM
                          spacing: Style.marginS
                          BotIcon {
                            cfg: deckCfg
                            kind: card2.ask.kind || "claude"
                            status: root.sessionStatus(card2.ask.session)
                            sizeScale: 0.7
                          }
                          NText {
                            text: (card2.ask.kind || "claude") + " · ws" + card2.ask.ws + (card2.ask.wsName ? "/" + card2.ask.wsName : "")
                            font.family: "Noto Sans Mono"
                            pointSize: Style.fontSizeS
                            color: Color.mOnSurfaceVariant
                            Layout.fillWidth: true
                          }
                          NText {
                            text: root.willWake(card2.ask) ? "waiting on you" : (root.sessionStatus(card2.ask.session) || "idle")
                            font.family: "Noto Sans Mono"
                            pointSize: Style.fontSizeS
                            color: root.willWake(card2.ask) ? Color.mPrimary : Color.mOnSurfaceVariant
                          }
                        }
                      }

                      // divider
                      Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 1
                        color: Qt.alpha(Color.mPrimary, 0.10)
                      }

                      // body
                      NText {
                        Layout.fillWidth: true
                        Layout.margins: Style.marginL
                        text: card2.ask.body || ""
                        font.family: "Noto Sans Mono"
                        pointSize: Style.fontSizeM
                        lineHeight: 1.5
                        wrapMode: Text.WordWrap
                        color: Color.mOnSurface
                      }
                    }
                  }

                  // one-click options — a Flow so buttons wrap to new lines, and
                  // each button caps to the row width and wraps its own text so a
                  // long answer never overruns.
                  RowLayout {
                    visible: card2.open && !card2.isNotify && (card2.ask.options || []).length > 0
                    Layout.fillWidth: true
                    spacing: Style.marginS
                    NText {
                      text: "answer"
                      Layout.alignment: Qt.AlignTop
                      Layout.topMargin: Style.marginS
                      font.family: "Noto Sans Mono"
                      pointSize: Style.fontSizeS
                      color: Color.mOnSurfaceVariant
                    }
                    Flow {
                      id: optionsFlow
                      Layout.fillWidth: true
                      spacing: Style.marginS
                      Repeater {
                        model: card2.ask.options || []
                        delegate: Rectangle {
                          required property var modelData
                          radius: Style.radiusXS
                          implicitWidth: Math.min(optTxt.implicitWidth + Style.marginL * 2, optionsFlow.width)
                          implicitHeight: optTxt.implicitHeight + Style.marginS * 2
                          color: optArea.containsMouse ? Qt.alpha(Color.mPrimary, 0.26) : Qt.alpha(Color.mPrimary, 0.15)
                          border.width: 1
                          border.color: Qt.alpha(Color.mPrimary, 0.32)
                          NText {
                            id: optTxt
                            anchors.centerIn: parent
                            width: parent.width - Style.marginL * 2
                            horizontalAlignment: Text.AlignHCenter
                            wrapMode: Text.WordWrap
                            text: modelData
                            font.family: "Noto Sans Mono"
                            font.weight: Style.fontWeightBold
                            pointSize: Style.fontSizeM
                            color: Color.mPrimary
                          }
                          MouseArea {
                            id: optArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.submitAnswer(card2.askId, modelData)
                          }
                        }
                      }
                    }
                  }

                  // notify: single acknowledgment
                  RowLayout {
                    visible: card2.isNotify
                    Layout.fillWidth: true
                    spacing: Style.marginS
                    Item {
                      Layout.fillWidth: true
                    }
                    NButton {
                      text: "Got it"
                      backgroundColor: Color.mPrimary
                      textColor: Color.mOnPrimary
                      buttonRadius: Style.radiusXS
                      onClicked: root.dismissAsk(card2.askId)
                    }
                  }

                  // reply + Trigger/Enqueue (reuses the exact v1 draft wiring)
                  RowLayout {
                    visible: !card2.isNotify
                    Layout.fillWidth: true
                    spacing: Style.marginS
                    NTextInput {
                      id: reply2
                      Layout.fillWidth: true
                      readOnly: !card2.open
                      placeholderText: card2.open ? "› reply…  (auto-saved, sent to harness stdin)" : "(completed without reply text)"
                      Component.onCompleted: {
                        text = card2.replying ? root.draftText : (card2.ask.answer || "");
                        if (card2.replying && card2.open)
                          inputItem.cursorPosition = text.length;
                      }
                      onTextChanged: {
                        if (card2.replying && card2.open && text !== root.draftText) {
                          root.draftText = text;
                          draftTimer.restart();
                        }
                      }
                      onEditingFinished: if (card2.replying && card2.open)
                        root.persistDraft()
                      onAccepted: if (card2.open)
                        root.submitAnswer(card2.askId, text)
                    }
                    // custom Trigger/Enqueue button (solid primary, matches spec)
                    Rectangle {
                      visible: card2.open
                      radius: Style.radiusXS
                      implicitHeight: trigTxt.implicitHeight + Style.marginM * 2
                      implicitWidth: trigTxt.implicitWidth + Style.marginL * 2
                      color: trigArea.containsMouse ? Qt.lighter(Color.mPrimary, 1.08) : Color.mPrimary
                      NText {
                        id: trigTxt
                        anchors.centerIn: parent
                        text: (root.willWake(card2.ask) ? "Trigger" : "Enqueue") + " ↵"
                        font.weight: Style.fontWeightBold
                        pointSize: Style.fontSizeM
                        color: Color.mOnPrimary
                      }
                      MouseArea {
                        id: trigArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.submitAnswer(card2.askId, reply2.text)
                      }
                    }
                  }

                  // note quick-tags (fully-rounded neutral pills)
                  RowLayout {
                    visible: !card2.isNotify
                    Layout.fillWidth: true
                    spacing: Style.marginS
                    NText {
                      text: "tag"
                      font.family: "Noto Sans Mono"
                      pointSize: Style.fontSizeS
                      color: Color.mOnSurfaceVariant
                    }
                    Repeater {
                      model: [
                        {
                          label: "working on it",
                          val: "working on it"
                        },
                        {
                          label: "later",
                          val: "later"
                        },
                        {
                          label: "your call",
                          val: "your call"
                        },
                        {
                          label: "need detail",
                          val: "need detail"
                        }
                      ]
                      delegate: Rectangle {
                        required property var modelData
                        readonly property bool on: card2.ask.note === modelData.val
                        radius: Style.radiusXS
                        implicitHeight: tagTxt.implicitHeight + Style.marginS * 2
                        implicitWidth: tagTxt.implicitWidth + Style.marginL * 2
                        color: on ? Qt.alpha(Color.mPrimary, 0.18) : "transparent"
                        border.width: 1
                        border.color: on ? Qt.alpha(Color.mPrimary, 0.50) : Qt.alpha(Color.mOnSurface, 0.14)
                        NText {
                          id: tagTxt
                          anchors.centerIn: parent
                          text: modelData.label
                          font.weight: Style.fontWeightBold
                          pointSize: Style.fontSizeS
                          color: on ? Color.mPrimary : Color.mOnSurfaceVariant
                        }
                        MouseArea {
                          anchors.fill: parent
                          cursorShape: Qt.PointingHandCursor
                          onClicked: root.noteAsk(card2.askId, card2.ask.note === modelData.val ? "" : modelData.val)
                        }
                      }
                    }
                  }
                }
              }
            }
        }
    }


    // ---- settings ----
    ColumnLayout {
      id: settingsCol
      visible: root.mode === "settings"
      anchors.fill: parent
      anchors.margins: Style.marginL
      spacing: Style.marginM

      RowLayout {
        id: settingsHeader
        Layout.fillWidth: true
        NText {
          text: "Battlestation Workspaces settings"
          pointSize: Style.fontSizeL
          font.weight: Style.fontWeightBold
          color: Color.mOnSurface
          Layout.fillWidth: true
        }
        NIconButton {
          icon: "close"
          onClicked: root.close()
        }
      }

      Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 1
        color: Color.mOutline
      }

      NScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        horizontalPolicy: ScrollBar.AlwaysOff

        Loader {
          id: settingsLoader
          width: parent.width
          // Loaded only in settings mode so the rename path stays light.
          source: root.mode === "settings" ? "Settings.qml" : ""
          onLoaded: if (item)
            item.pluginApi = root.pluginApi
        }
      }

      RowLayout {
        id: chrome
        Layout.fillWidth: true
        spacing: Style.marginM
        Item {
          Layout.fillWidth: true
        }
        NButton {
          text: "Close"
          outlined: true
          onClicked: root.close()
        }
        NButton {
          text: "Apply"
          icon: "check"
          backgroundColor: Color.mPrimary
          textColor: Color.mOnPrimary
          onClicked: root.settingsApply()
        }
      }
    }
  }

  Component.onCompleted: {
    // Load the persisted filter FIRST (it shapes the membership), then
    // seed the row model, then SNAPSHOT the panel height (imperative on
    // purpose: a binding would track the queue and re-animate the frame —
    // anti-jank rule 2).
    loadHideDelivered();
    syncAsks();
    if (root.asksMode)
      asksPanelHeight = computeAsksHeight();
    if (root.mode === "rename") {
      if (main)
        renameInput.text = main.pendingRenameName;
      Qt.callLater(function () {
        renameInput.inputItem.forceActiveFocus();
        renameInput.inputItem.selectAll();
      });
    }
  }
}
