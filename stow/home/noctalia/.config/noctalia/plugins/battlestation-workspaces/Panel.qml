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
// collapses the expanded reply area so every open row has the same height —
// which is what makes the slot arithmetic below trivial. The dragged row's
// model is untouched until release (rebuilding delegates would destroy the
// very delegate being dragged); instead a floating proxy follows the pointer
// and a drop line marks the target slot, and release commits the whole
// open-id list via `asks order set` — the same gesture-agnostic payload as
// any other reorder.
//
// Two anti-jank rules shape the asks mode (both were live bugs — clicking
// around used to re-animate the whole panel):
//  1. IDENTITY-STABLE ROWS. The row Repeater's model is `askIds` — the bare
//     id sequence, reassigned ONLY when the sequence itself changes
//     (post/dismiss/state moves/reorder). Everything else about a row flows
//     through the `askById` lookup map, so a content change (a note, an
//     urgency bump, the stream echoing our own draft autosave every ~1.5s
//     while typing) updates bindings IN PLACE and never destroys a delegate
//     — the same displayList/lookup-maps pattern BarWidget uses for pills.
//  2. FIXED PANEL GEOMETRY. contentPreferredHeight is snapshotted ONCE at
//     open (sized to the queue, capped) and never re-bound: the SmartPanel
//     animates geometry changes, so a height that tracked the content made
//     every expand/collapse read as a panel re-open. Rows scroll INSIDE
//     (NScrollView — the settings mode's pattern) and expansion changes
//     nothing about the panel's frame.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
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

  // SmartPanel contract. Asks is the widest — a triage row needs title, meta
  // and its action cluster side by side without fighting; settings is sized
  // to its full content (SmartPanel clamps to the screen, and the scroll
  // view only kicks in if it can't fit); rename hugs its content.
  readonly property var geometryPlaceholder: panelContainer
  readonly property bool allowAttach: true
  property real contentPreferredWidth: (mode === "settings" ? 580 : mode === "asks" ? 880 : 320) * Style.uiScaleRatio
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
    return Math.min(620, 108 + n * 46 + 190) * Style.uiScaleRatio;
  }
  property real contentPreferredHeight: (mode === "settings" ? _settingsHeight : mode === "asks" ? asksPanelHeight : renameCol.implicitHeight + Style.marginL * 2)

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
  // "Hide delivered" (default ON): a delivered ask is finished business —
  // asked, answered, and the answer has reached its asker (delivered_at,
  // stamped only by the asker's own MCP collection; contract in
  // ctl/src/lib.rs). Hiding on delivery IS the auto-fade: the row vanishes
  // the moment the agent collects, no timer. Persisted in pluginSettings
  // so the choice survives panel opens and shell restarts.
  property bool hideDelivered: true
  onHideDeliveredChanged: syncAsks()
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
    running: root.mode === "asks"
    repeat: true
    onTriggered: root.nowS = Date.now() / 1000
  }

  // ---- drag-handle reorder state ----------------------------------------------
  // Captured in rowsCol coordinates (the scrolling inner column — slot math
  // is content-space, immune to the scroll offset) at ACTIVATION (first
  // move past the threshold), one event after the press collapsed the
  // reply areas — by then the column has relaid out and every open row is
  // slot-height uniform. An id-sequence change mid-drag rebuilds the
  // delegates and silently cancels the drag (release finds no state to
  // commit) — accepted; asks changing under an in-flight drag is rare and
  // the store stays authoritative. Content-only stream changes leave the
  // drag untouched (identity-stable rows).
  property int dragId: -1
  property int dragFrom: -1
  property int dropIndex: -1
  property real dragSlotH: 0
  property real dragFirstY: 0
  property string dragTitle: ""
  property real proxyY: 0
  property real dropLineY: 0
  property var pressRow: null
  property real pressY0: 0

  function openIds() {
    var ids = [];
    for (var i = 0; i < asksRows.length; i++)
      if (asksRows[i].state === "open")
        ids.push(asksRows[i].id);
    return ids;
  }
  function dragPress(row, area, mx, my) {
    collapseExpanded(); // persists any draft; uniform slot heights before any geometry is read
    pressRow = row;
    pressY0 = area.mapToItem(rowsCol, mx, my).y;
  }
  function dragMove(row, area, mx, my) {
    var p = area.mapToItem(rowsCol, mx, my);
    if (dragId < 0) {
      if (pressRow !== row || Math.abs(p.y - pressY0) < 6)
        return; // activation threshold: a sloppy click must not reorder
      dragId = row.askId;
      dragFrom = row.index; // open rows lead the resolved order, so model index == open slot
      dragSlotH = row.height;
      dragFirstY = row.y - row.index * (dragSlotH + rowsCol.spacing);
      dragTitle = row.ask.title || "";
    }
    var n = openIds().length;
    var pitch = dragSlotH + rowsCol.spacing;
    dropIndex = Math.max(0, Math.min(n - 1, Math.round((p.y - dragFirstY - dragSlotH / 2) / pitch)));
    proxyY = area.mapToItem(panelContainer, mx, my).y;
    // The line sits above the target slot when moving up, below it when
    // moving down (the removal shifts everything after the source up one).
    // Mapped rowsCol -> panelContainer at event time, so the current scroll
    // offset is baked in (the overlay is a panelContainer sibling).
    var edge = dropIndex <= dragFrom ? dragFirstY + dropIndex * pitch : dragFirstY + dropIndex * pitch + dragSlotH + rowsCol.spacing;
    dropLineY = rowsCol.mapToItem(panelContainer, 0, edge).y - rowsCol.spacing / 2;
  }
  function dragRelease() {
    if (dragId >= 0 && dropIndex >= 0 && dropIndex !== dragFrom) {
      var ids = openIds();
      var from = ids.indexOf(dragId);
      if (from >= 0) {
        ids.splice(from, 1);
        ids.splice(dropIndex, 0, dragId);
        bsctl(["asks", "order", "set"].concat(ids.map(String)));
      }
    }
    dragReset();
  }
  function dragReset() {
    dragId = -1;
    dragFrom = -1;
    dropIndex = -1;
    pressRow = null;
    dragTitle = "";
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

    // ---- asks ----
    ColumnLayout {
      id: asksCol
      visible: root.mode === "asks"
      anchors.fill: parent
      anchors.margins: Style.marginL
      spacing: Style.marginM

      RowLayout {
        Layout.fillWidth: true
        NText {
          text: "The Deck"
          pointSize: Style.fontSizeL
          font.weight: Style.fontWeightBold
          color: Color.mOnSurface
          Layout.fillWidth: true
        }
        NCheckbox {
          label: "Inject"
          labelSize: Style.fontSizeS
          checked: root.injectOn
          onToggled: checked => root.setInject(checked)
          // Standing per-turn nudge to agents to USE the Deck (see setInject).
          Layout.fillWidth: false
        }
        NCheckbox {
          label: "Background retrigger"
          labelSize: Style.fontSizeS
          checked: root.retriggerOn
          onToggled: checked => root.setRetrigger(checked)
          // Stop-hook backstop: re-prompt an unwatched turn that ended with an
          // unposted question (see setRetrigger).
          Layout.fillWidth: false
        }
        NCheckbox {
          label: "Hide delivered"
          labelSize: Style.fontSizeS
          checked: root.hideDelivered
          onToggled: checked => root.setHideDelivered(checked)
          // NCheckbox is a fill-width RowLayout with an internal spacer that
          // shoves the box to its far edge — stretched beside the fill-width
          // title, its label strands mid-header. Compact keeps label + box
          // together as one right-aligned unit.
          Layout.fillWidth: false
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

      // Rows scroll INSIDE the fixed panel frame (anti-jank rule 2): wheel
      // scrolling via NScrollView (the settings mode's pattern) — no
      // ListView, so there is no interactive-flick to fight the handle drag.
      NScrollView {
        id: asksScroll
        Layout.fillWidth: true
        Layout.fillHeight: true
        horizontalPolicy: ScrollBar.AlwaysOff

        ColumnLayout {
          id: rowsCol
          width: parent.width
          spacing: Style.marginM

          Item {
            visible: root.askIds.length === 0
            Layout.fillWidth: true
            implicitHeight: Math.max(60, asksScroll.height * 0.85)
            NText {
              anchors.centerIn: parent
              text: "the Deck is clear"
              color: Color.mOnSurfaceVariant
            }
          }

          Repeater {
            // Rows in the stream's RESOLVED order (open in the human's
            // order, then FIFO, answered tail last) — never re-sorted here.
            // The model is the ID SEQUENCE only; content flows through
            // askById so delegates survive every content-only change
            // (anti-jank rule 1).
            model: root.mode === "asks" ? root.askIds : []

            delegate: ColumnLayout {
              id: askRow
              required property var modelData
              required property int index
              readonly property int askId: modelData
              // ({}) fallback covers the one-frame gap while a vanished id's
              // delegate awaits destruction (see syncAsks).
              readonly property var ask: root.askById[String(modelData)] || ({})
              readonly property bool replying: root.expandedId === askId
              readonly property bool open: ask.state === "open"
              readonly property bool dragging: root.dragId === askId
              // An FYI's lifecycle is seen -> gone: notify rows swap the
              // whole reply machinery for a single "Got it" (dismiss).
              readonly property bool isNotify: ask.type === "notify"

          Layout.fillWidth: true
          spacing: Style.marginXS
          // The original stays in place, dimmed, while its proxy rides the
          // pointer — the model must not move until release (see header).
          opacity: dragging ? 0.35 : 1.0

          // Header wrapper: a MouseArea UNDER the row content makes the
          // whole row body click-to-expand — buttons and the drag handle
          // sit above it and keep their events; title, meta and gaps fall
          // through. The tinted backdrop is the clickability affordance.
          Item {
            Layout.fillWidth: true
            implicitHeight: headerRow.implicitHeight

            Rectangle {
              anchors.fill: parent
              anchors.leftMargin: -Style.marginXS
              anchors.rightMargin: -Style.marginXS
              radius: Style.radiusXS
              color: rowArea.containsMouse ? Qt.alpha(Color.mOnSurface, 0.06) : "transparent"
            }
            MouseArea {
              id: rowArea
              anchors.fill: parent
              hoverEnabled: true
              onClicked: root.toggleExpand(askRow)
            }

            RowLayout {
              id: headerRow
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              spacing: Style.marginS

              // The drag handle. Fixed-width slot on every row (answered rows
              // keep the space so title columns align) but only open rows show
              // the grip and accept the drag.
              Item {
                Layout.preferredWidth: 18
                Layout.preferredHeight: 22
                NText {
                  anchors.centerIn: parent
                  text: "≡"
                  visible: askRow.open
                  color: handleArea.containsMouse || askRow.dragging ? Color.mOnSurface : Qt.alpha(Color.mOnSurface, 0.35)
                }
                MouseArea {
                  id: handleArea
                  anchors.fill: parent
                  enabled: askRow.open
                  hoverEnabled: true
                  preventStealing: true
                  cursorShape: askRow.dragging ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                  onPressed: mouse => root.dragPress(askRow, handleArea, mouse.x, mouse.y)
                  onPositionChanged: mouse => {
                    if (pressed)
                      root.dragMove(askRow, handleArea, mouse.x, mouse.y);
                  }
                  onReleased: root.dragRelease()
                  onCanceled: root.dragReset()
                }
              }

              Rectangle {
                width: 10
                height: 10
                radius: 5
                color: root.dotColor(askRow.ask.blocking === true)
                opacity: askRow.open ? 1.0 : 0.4
              }

              // Type at a glance (names verified against the Tabler map):
              // question-mark / eye (review) / info-circle (notify, in the
              // secondary accent so FYIs read different without shouting).
              NIcon {
                icon: askRow.isNotify ? "info-circle" : askRow.ask.type === "review" ? "eye" : "question-mark"
                pointSize: Style.fontSizeS
                color: askRow.isNotify ? Color.mSecondary : Color.mOnSurfaceVariant
                opacity: askRow.open ? 1.0 : 0.5
              }

              ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                NText {
                  // NText is a Text with elide: ElideRight by default — width
                  // does the truncation now that the panel is wide.
                  text: askRow.ask.title
                  font.weight: Style.fontWeightBold
                  color: Color.mOnSurface
                  opacity: askRow.open ? 1.0 : 0.6
                  Layout.fillWidth: true
                }
                RowLayout {
                  // Urgency as its own colored segment (elide + rich text
                  // don't mix on one Text, so the token is a sibling).
                  Layout.fillWidth: true
                  spacing: 0
                  NText {
                    text: root.urgencyLabel(askRow.ask.urgency)
                    pointSize: Style.fontSizeXS
                    font.weight: askRow.ask.urgency === "high" ? Style.fontWeightBold : Style.fontWeightRegular
                    color: root.urgencyTextColor(askRow.ask.urgency)
                    opacity: askRow.open ? 1.0 : 0.6
                  }
                  NText {
                    text: "  ·  " + root.askMeta(askRow.ask)
                    pointSize: Style.fontSizeXS
                    color: Color.mOnSurfaceVariant
                    Layout.fillWidth: true
                  }
                }
              }

              NButton {
                visible: (askRow.ask.session || "").length > 0
                text: "Jump"
                outlined: true
                onClicked: root.jumpToAsk(askRow.ask.session)
              }
              NButton {
                visible: askRow.ask.state === "answered"
                text: "Reopen"
                outlined: true
                onClicked: root.reopenAsk(askRow.askId)
              }
              NIconButton {
                icon: "close"
                onClicked: root.dismissAsk(askRow.askId)
              }
            }
          }

          // Expanded detail: body, one-click options, free-text reply, and
          // the note quick-tags (the human half of the queue conversation —
          // visible to every agent via the stream). Answered rows expand
          // too — a read-only view of the answer, with Reopen as the way
          // back to editing.
          ColumnLayout {
            visible: askRow.replying
            Layout.fillWidth: true
            Layout.leftMargin: 18 + 10 + Style.marginS * 2
            spacing: Style.marginXS

            NText {
              visible: (askRow.ask.body || "").length > 0
              text: askRow.ask.body || ""
              wrapMode: Text.WordWrap
              color: Color.mOnSurfaceVariant
              Layout.fillWidth: true
            }

            RowLayout {
              visible: askRow.open && !askRow.isNotify && (askRow.ask.options || []).length > 0
              Layout.fillWidth: true
              spacing: Style.marginXS
              NText {
                text: "answer:"
                pointSize: Style.fontSizeXS
                color: Color.mOnSurfaceVariant
              }
              Repeater {
                model: askRow.ask.options || []
                delegate: NButton {
                  required property var modelData
                  text: modelData
                  backgroundColor: Color.mPrimary
                  textColor: Color.mOnPrimary
                  onClicked: root.submitAnswer(askRow.askId, modelData)
                }
              }
            }

            // Notify expansion: body above, one acknowledgment below — no
            // reply input, no Done, no draft wiring (the panel's draft state
            // stays inert: no input exists to feed it, and persistDraft's
            // unchanged-text guard makes the collapse paths no-ops).
            RowLayout {
              visible: askRow.isNotify
              Layout.fillWidth: true
              spacing: Style.marginXS
              Item {
                Layout.fillWidth: true
              }
              NButton {
                text: "Got it"
                backgroundColor: Color.mPrimary
                textColor: Color.mOnPrimary
                onClicked: root.dismissAsk(askRow.askId)
              }
            }

            RowLayout {
              visible: !askRow.isNotify
              Layout.fillWidth: true
              spacing: Style.marginXS
              NTextInput {
                id: replyInput
                Layout.fillWidth: true
                readOnly: !askRow.open // answered rows show, Reopen edits
                placeholderText: askRow.open ? "Reply… (auto-saved)" : "(completed without reply text)"
                // Restore the panel-held draft after any delegate rebuild
                // (stream changes — including the echo of our own auto-save
                // — recreate this input mid-typing); track keystrokes back
                // into it and restart the debounce while this row is the
                // expanded one. Cursor to the end after a restore, so a
                // rebuild between keystrokes never teleports the caret.
                Component.onCompleted: {
                  text = askRow.replying ? root.draftText : (askRow.ask.answer || "");
                  if (askRow.replying && askRow.open)
                    inputItem.cursorPosition = text.length;
                }
                onTextChanged: {
                  if (askRow.replying && askRow.open && text !== root.draftText) {
                    root.draftText = text;
                    draftTimer.restart();
                  }
                }
                // Blur persists (editingFinished fires on focus loss and on
                // Enter; the dedupe in persistDraft makes the Enter case a
                // no-op after submitAnswer's cancel).
                onEditingFinished: if (askRow.replying && askRow.open)
                  root.persistDraft()
                onAccepted: if (askRow.open)
                  root.submitAnswer(askRow.askId, text)
              }
              NButton {
                // Reply AND complete (empty text = ack-only completion); the
                // draft needs no button: it auto-saves. The label is the
                // outcome: "Trigger" when the idle asker will be woken to
                // collect the answer, "Enqueue" when a busy/blocking asker
                // collects it on its own (contract: lib.rs "DELIVERY vs WAKE").
                visible: askRow.open
                text: root.willWake(askRow.ask) ? "Trigger" : "Enqueue"
                backgroundColor: Color.mPrimary
                textColor: Color.mOnPrimary
                onClicked: root.submitAnswer(askRow.askId, replyInput.text)
              }
            }

            RowLayout {
              Layout.fillWidth: true
              spacing: Style.marginXS
              NText {
                text: "tag:"
                pointSize: Style.fontSizeXS
                color: Color.mOnSurfaceVariant
              }
              NButton {
                text: "working on it"
                outlined: true
                onClicked: root.noteAsk(askRow.askId, "working on it")
              }
              NButton {
                text: "later"
                outlined: true
                onClicked: root.noteAsk(askRow.askId, "later")
              }
              NButton {
                visible: (askRow.ask.note || "").length > 0
                text: "clear"
                outlined: true
                onClicked: root.noteAsk(askRow.askId, "")
              }
            }
          }
        }
      }
        }
      }
    }

    // ---- drag overlay (asks mode) ----
    // Floating proxy + drop line live OUTSIDE asksCol: a ColumnLayout lays
    // out every child, so free-positioned items must be siblings. Positions
    // are computed in dragMove (event-time mapping, not bindings — the
    // panel can re-center mid-drag and stale bindings would lie).
    Rectangle {
      visible: root.dragId >= 0
      x: asksCol.x
      y: root.proxyY - height / 2
      width: asksCol.width
      height: 30
      radius: Style.radiusXS
      color: Color.mSurfaceVariant
      border.color: Color.mPrimary
      border.width: 1
      opacity: 0.92
      z: 100
      RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Style.marginS
        anchors.rightMargin: Style.marginS
        spacing: Style.marginS
        NText {
          text: "≡"
          color: Color.mOnSurface
        }
        NText {
          text: root.dragTitle
          font.weight: Style.fontWeightBold
          color: Color.mOnSurface
          Layout.fillWidth: true
        }
      }
    }
    Rectangle {
      visible: root.dragId >= 0 && root.dropIndex !== root.dragFrom
      x: asksCol.x
      y: root.dropLineY - height / 2
      width: asksCol.width
      height: 2
      radius: 1
      color: Color.mPrimary
      z: 99
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
    if (root.mode === "asks")
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
