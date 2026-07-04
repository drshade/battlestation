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
// Reordering is a drag HANDLE (the grip at each open row's left edge), not a
// full-row drag: row bodies keep their clicks, and pressing the handle first
// collapses the expanded reply area so every open row has the same height —
// which is what makes the slot arithmetic below trivial. The dragged row's
// model is untouched until release (a Repeater rebuilds delegates on model
// change, so live reordering would destroy the very delegate being dragged);
// instead a floating proxy follows the pointer and a drop line marks the
// target slot, and release commits the whole open-id list via
// `asks order set` — the same gesture-agnostic payload as any other reorder.
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
  property real contentPreferredWidth: (mode === "settings" ? 580 : mode === "asks" ? 800 : 320) * Style.uiScaleRatio
  // Full natural height of the settings column (header + separator + 3 row gaps +
  // top/bottom margins + the settings content). No artificial cap: SmartPanel
  // clamps to the screen, and the scroll view only kicks in if it can't fit.
  readonly property real _settingsHeight: settingsLoader.implicitHeight + settingsHeader.implicitHeight + chrome.implicitHeight + 1 + Style.marginM * 3 + Style.marginL * 2
  property real contentPreferredHeight: (mode === "settings" ? _settingsHeight : mode === "asks" ? asksCol.implicitHeight + Style.marginL * 2 : renameCol.implicitHeight + Style.marginL * 2)

  // ---- asks state -------------------------------------------------------------
  readonly property var asksRows: (main && main.asksRows) ? main.asksRows : []
  // Single-expansion: at most one row's reply area is open (triage is one
  // ask at a time, and collapsing everything at drag start is then trivial).
  property int expandedId: -1
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
  // Captured in asksCol coordinates at ACTIVATION (first move past the
  // threshold), one event after the press collapsed the reply areas — by
  // then the column has relaid out and every open row is slot-height
  // uniform. A stream update mid-drag rebuilds the delegates and silently
  // cancels the drag (release finds no state to commit) — accepted; asks
  // changing under an in-flight drag is rare and the store stays authoritative.
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
    expandedId = -1; // uniform slot heights before any geometry is read
    pressRow = row;
    pressY0 = area.mapToItem(asksCol, mx, my).y;
  }
  function dragMove(row, area, mx, my) {
    var p = area.mapToItem(asksCol, mx, my);
    if (dragId < 0) {
      if (pressRow !== row || Math.abs(p.y - pressY0) < 6)
        return; // activation threshold: a sloppy click must not reorder
      dragId = row.modelData.id;
      dragFrom = row.index; // open rows lead the resolved order, so model index == open slot
      dragSlotH = row.height;
      dragFirstY = row.y - row.index * (dragSlotH + asksCol.spacing);
      dragTitle = row.modelData.title;
    }
    var n = openIds().length;
    var pitch = dragSlotH + asksCol.spacing;
    dropIndex = Math.max(0, Math.min(n - 1, Math.round((p.y - dragFirstY - dragSlotH / 2) / pitch)));
    proxyY = area.mapToItem(panelContainer, mx, my).y;
    // The line sits above the target slot when moving up, below it when
    // moving down (the removal shifts everything after the source up one).
    var edge = dropIndex <= dragFrom ? dragFirstY + dropIndex * pitch : dragFirstY + dropIndex * pitch + dragSlotH + asksCol.spacing;
    dropLineY = asksCol.y + edge - asksCol.spacing / 2;
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
  function urgencyColor(u) {
    return u === "high" ? Color.mError : u === "medium" ? Color.mTertiary : Qt.alpha(Color.mOnSurface, 0.35);
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
    var parts = [];
    if (r.kind)
      parts.push(r.kind);
    if (r.ws !== null && r.ws !== undefined)
      parts.push("ws " + r.ws);
    parts.push(fmtAge(r.created));
    if (r.estimate_min)
      parts.push("~" + r.estimate_min + "m of you");
    if (r.state === "answered")
      parts.push("answered — awaiting pickup");
    if (r.note)
      parts.push("“" + r.note + "”");
    return parts.join("  ·  ");
  }

  // ---- asks actions (direct argv: no shell, no quoting) -----------------------
  function bsctl(args) {
    askProc.command = [Quickshell.env("HOME") + "/.local/bin/bsctl"].concat(args);
    askProc.running = true;
  }
  function answerAsk(id, text) {
    if (text.length > 0)
      bsctl(["asks", "answer", String(id), text]);
  }
  function noteAsk(id, text) {
    bsctl(["asks", "note", String(id), text]);
  }
  function dismissAsk(id) {
    bsctl(["asks", "dismiss", String(id)]);
  }
  function jumpToAsk(ws) {
    bsctl(["ws", "focus", "--ws-id", String(ws)]);
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
    if (pluginApi)
      pluginApi.closePanel(pluginApi.panelOpenScreen);
  }

  Process {
    id: renameProc
  }
  Process {
    id: askProc
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
      anchors.centerIn: parent
      width: parent.width - Style.marginL * 2
      spacing: Style.marginM

      RowLayout {
        Layout.fillWidth: true
        NText {
          text: "Asks"
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

      NText {
        visible: root.asksRows.length === 0
        text: "no asks — all clear"
        color: Color.mOnSurfaceVariant
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: Style.marginS
        Layout.bottomMargin: Style.marginS
      }

      Repeater {
        // Rows in the stream's RESOLVED order (open in the human's order,
        // then FIFO, answered tail last) — never re-sorted here.
        model: root.mode === "asks" ? root.asksRows : []

        delegate: ColumnLayout {
          id: askRow
          required property var modelData
          required property int index
          readonly property bool replying: root.expandedId === modelData.id
          readonly property bool open: modelData.state === "open"
          readonly property bool answerable: open && (modelData.type === "question" || modelData.type === "review")
          readonly property bool dragging: root.dragId === modelData.id

          Layout.fillWidth: true
          spacing: Style.marginXS
          // The original stays in place, dimmed, while its proxy rides the
          // pointer — the model must not move until release (see header).
          opacity: dragging ? 0.35 : 1.0

          RowLayout {
            Layout.fillWidth: true
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
              color: root.urgencyColor(askRow.modelData.urgency)
              opacity: askRow.open ? 1.0 : 0.4
            }

            ColumnLayout {
              Layout.fillWidth: true
              spacing: 0
              NText {
                // NText is a Text with elide: ElideRight by default — width
                // does the truncation now that the panel is wide.
                text: askRow.modelData.title
                font.weight: Style.fontWeightBold
                color: Color.mOnSurface
                opacity: askRow.open ? 1.0 : 0.6
                Layout.fillWidth: true
              }
              NText {
                text: root.askMeta(askRow.modelData)
                pointSize: Style.fontSizeXS
                color: Color.mOnSurfaceVariant
                Layout.fillWidth: true
              }
            }

            NButton {
              visible: askRow.modelData.ws !== null && askRow.modelData.ws !== undefined
              text: "Jump"
              outlined: true
              onClicked: root.jumpToAsk(askRow.modelData.ws)
            }
            NButton {
              visible: askRow.answerable
              text: askRow.replying ? "Hide" : "Reply"
              outlined: !askRow.replying
              onClicked: root.expandedId = askRow.replying ? -1 : askRow.modelData.id
            }
            NIconButton {
              icon: "close"
              onClicked: root.dismissAsk(askRow.modelData.id)
            }
          }

          // Expanded detail: body, one-click options, free-text reply, and
          // the note quick-tags (the human half of the queue conversation —
          // visible to every agent via the stream).
          ColumnLayout {
            visible: askRow.replying && askRow.open
            Layout.fillWidth: true
            Layout.leftMargin: 18 + 10 + Style.marginS * 2
            spacing: Style.marginXS

            NText {
              visible: (askRow.modelData.body || "").length > 0
              text: askRow.modelData.body || ""
              wrapMode: Text.WordWrap
              color: Color.mOnSurfaceVariant
              Layout.fillWidth: true
            }

            RowLayout {
              visible: (askRow.modelData.options || []).length > 0
              Layout.fillWidth: true
              spacing: Style.marginXS
              NText {
                text: "answer:"
                pointSize: Style.fontSizeXS
                color: Color.mOnSurfaceVariant
              }
              Repeater {
                model: askRow.modelData.options || []
                delegate: NButton {
                  required property var modelData
                  text: modelData
                  backgroundColor: Color.mPrimary
                  textColor: Color.mOnPrimary
                  onClicked: root.answerAsk(askRow.modelData.id, modelData)
                }
              }
            }

            RowLayout {
              Layout.fillWidth: true
              spacing: Style.marginXS
              NTextInput {
                id: replyInput
                Layout.fillWidth: true
                placeholderText: "Reply…"
                onAccepted: root.answerAsk(askRow.modelData.id, text)
              }
              NButton {
                text: "Send"
                backgroundColor: Color.mPrimary
                textColor: Color.mOnPrimary
                onClicked: root.answerAsk(askRow.modelData.id, replyInput.text)
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
                onClicked: root.noteAsk(askRow.modelData.id, "working on it")
              }
              NButton {
                text: "later"
                outlined: true
                onClicked: root.noteAsk(askRow.modelData.id, "later")
              }
              NButton {
                visible: (askRow.modelData.note || "").length > 0
                text: "clear"
                outlined: true
                onClicked: root.noteAsk(askRow.modelData.id, "")
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
