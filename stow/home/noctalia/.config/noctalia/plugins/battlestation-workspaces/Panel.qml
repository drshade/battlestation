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

  // SmartPanel contract. Settings is wider and sized to its full content
  // (SmartPanel clamps to the screen, and the scroll view only kicks in if it
  // can't fit); rename and asks hug their content.
  readonly property var geometryPlaceholder: panelContainer
  readonly property bool allowAttach: true
  property real contentPreferredWidth: (mode === "settings" ? 580 : mode === "asks" ? 480 : 320) * Style.uiScaleRatio
  // Full natural height of the settings column (header + separator + 3 row gaps +
  // top/bottom margins + the settings content). No artificial cap: SmartPanel
  // clamps to the screen, and the scroll view only kicks in if it can't fit.
  readonly property real _settingsHeight: settingsLoader.implicitHeight + settingsHeader.implicitHeight + chrome.implicitHeight + 1 + Style.marginM * 3 + Style.marginL * 2
  property real contentPreferredHeight: (mode === "settings" ? _settingsHeight : mode === "asks" ? asksCol.implicitHeight + Style.marginL * 2 : renameCol.implicitHeight + Style.marginL * 2)

  // ---- asks state -------------------------------------------------------------
  readonly property var asksRows: (main && main.asksRows) ? main.asksRows : []
  // Age text ticks while the panel is up (rows only re-render on stream
  // changes; age would otherwise freeze at open time).
  property real nowS: Date.now() / 1000
  Timer {
    interval: 30000
    running: root.mode === "asks"
    repeat: true
    onTriggered: root.nowS = Date.now() / 1000
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
  // Reorder = the human's order file, whole-list semantics: all OPEN ids with
  // the moved one shifted a slot (edges no-op). Answered rows aren't
  // reorderable — they're past triage.
  function moveAsk(id, delta) {
    var ids = [];
    for (var i = 0; i < asksRows.length; i++)
      if (asksRows[i].state === "open")
        ids.push(asksRows[i].id);
    var from = ids.indexOf(id);
    var to = from + delta;
    if (from < 0 || to < 0 || to >= ids.length)
      return;
    ids.splice(from, 1);
    ids.splice(to, 0, id);
    bsctl(["asks", "order", "set"].concat(ids.map(String)));
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
          property bool replying: false
          readonly property bool open: modelData.state === "open"
          readonly property bool answerable: open && (modelData.type === "question" || modelData.type === "review")

          Layout.fillWidth: true
          spacing: Style.marginXS

          RowLayout {
            Layout.fillWidth: true
            spacing: Style.marginS

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
                // No elide on NText: cap like pillLabel does.
                text: String(askRow.modelData.title).length > 60 ? String(askRow.modelData.title).substring(0, 60) + "…" : askRow.modelData.title
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
              visible: askRow.open
              text: "↑"
              outlined: true
              onClicked: root.moveAsk(askRow.modelData.id, -1)
            }
            NButton {
              visible: askRow.open
              text: "↓"
              outlined: true
              onClicked: root.moveAsk(askRow.modelData.id, 1)
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
              onClicked: askRow.replying = !askRow.replying
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
            Layout.leftMargin: 10 + Style.marginS
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
