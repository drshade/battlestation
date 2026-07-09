// Bar-attached plugin panel with exactly one job: host the widget's
// Settings.qml (mic picker etc.) so a right-click on the pill opens it in
// place — the same hang-off-the-bar blob battlestation-workspaces uses for
// its settings mode, minus the other modes. The SmartPanel contract
// (geometryPlaceholder / allowAttach / contentPreferred*) is what makes
// Noctalia attach and animate it like a native panel.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import qs.Commons
import qs.Widgets

Item {
  id: root

  property var pluginApi: null

  // SmartPanel contract.
  readonly property var geometryPlaceholder: panelContainer
  readonly property bool allowAttach: true
  property real contentPreferredWidth: 580 * Style.uiScaleRatio
  property real contentPreferredHeight: settingsLoader.implicitHeight + settingsHeader.implicitHeight + chrome.implicitHeight + 1 + Style.marginM * 3 + Style.marginL * 2

  function settingsApply() {
    if (settingsLoader.item && settingsLoader.item.saveSettings)
      settingsLoader.item.saveSettings();
    close();
  }

  function close() {
    if (pluginApi)
      pluginApi.closePanel(pluginApi.panelOpenScreen);
  }

  Item {
    id: panelContainer
    anchors.fill: parent

    ColumnLayout {
      anchors.fill: parent
      anchors.margins: Style.marginL
      spacing: Style.marginM

      RowLayout {
        id: settingsHeader
        Layout.fillWidth: true
        NText {
          text: "Battlestation Dictation settings"
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
        implicitHeight: 1
        color: Color.mOutline
      }

      NScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        // Pins content to the viewport width (preventHorizontalScroll) —
        // without it the column grows to its natural width and overflows
        // the panel's right edge.
        horizontalPolicy: ScrollBar.AlwaysOff
        Loader {
          id: settingsLoader
          width: parent.width
          source: "Settings.qml"
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
}
