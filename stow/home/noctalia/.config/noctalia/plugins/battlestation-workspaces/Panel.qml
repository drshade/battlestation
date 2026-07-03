// Bar-attached plugin panel, used for two things so both share the same
// hang-off-the-bar blob, animation and exclusive keyboard:
//   mode "rename"   -> a single text field to rename a workspace
//   mode "settings" -> the widget's Settings.qml hosted with Apply/Close
// The mode and the rename target are staged on the plugin's mainInstance before
// the panel is opened (the panel content is recreated on every open).
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
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
  // can't fit); rename hugs its content.
  readonly property var geometryPlaceholder: panelContainer
  readonly property bool allowAttach: true
  property real contentPreferredWidth: (mode === "settings" ? 580 : 320) * Style.uiScaleRatio
  // Full natural height of the settings column (header + separator + 3 row gaps +
  // top/bottom margins + the settings content). No artificial cap: SmartPanel
  // clamps to the screen, and the scroll view only kicks in if it can't fit.
  readonly property real _settingsHeight: settingsLoader.implicitHeight + settingsHeader.implicitHeight + chrome.implicitHeight + 1 + Style.marginM * 3 + Style.marginL * 2
  property real contentPreferredHeight: (mode === "settings" ? _settingsHeight : renameCol.implicitHeight + Style.marginL * 2)

  function renameSubmit() {
    // Positional args keep the name opaque to the shell; bsctl ws escapes it and
    // resets to the number when empty.
    renameProc.command = ["sh", "-c", "$HOME/.local/bin/bsctl ws rename \"$1\" \"$2\"", "sh", String(wsId), renameInput.text];
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
