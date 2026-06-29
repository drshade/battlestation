// One workspace pill: the capsule (filled or outlined), the "<n> - <name>"
// label, and a row of animated BotIcons (one per Claude instance in icons mode).
import QtQuick
import qs.Commons
import qs.Widgets
import qs.Services.Compositor

Item {
  id: cell

  property var ws: null            // the compositor workspace model
  property var cfg: null
  property var instances: []       // statuses of Claude instances here
  property bool occupied: false
  property bool shown: true
  property int position: 0         // display position (1-based); shown instead of the raw id

  readonly property bool active: ws && ws.isFocused === true
  property int pokeNonce: 0

  function poke() {
    pokeNonce++;
  }
  onActiveChanged: if (active)
    poke()

  // Pill background follows the workspace state; Claude status shows via the bots.
  function bg() {
    if (active)
      return cfg.wsBg("focused");
    if (occupied)
      return cfg.wsBg("occupied");
    return cfg.wsBg("empty");
  }
  function fg() {
    if (active)
      return cfg.wsOn("focused");
    if (occupied)
      return cfg.wsOn("occupied");
    return cfg.wsOn("empty");
  }
  // In outline mode the label takes the pill's colour (mOnSurface if transparent).
  function lineText() {
    const c = bg();
    return (c === "transparent") ? Color.mOnSurface : c;
  }

  visible: shown
  height: cfg.barHeight
  width: shown ? Math.max(cfg.d * (active ? 2.2 : 1), Math.round(content.implicitWidth + cfg.d * (cfg.outline ? 1.35 : 0.6))) : 0

  Behavior on width {
    NumberAnimation {
      duration: Style.animationNormal
      easing.type: Easing.OutBack
    }
  }

  Rectangle {
    id: pill
    anchors.centerIn: parent
    width: parent.width
    height: cfg.d
    radius: Style.radiusM
    color: cfg.outline ? "transparent" : cell.bg()
    border.width: cfg.outline ? Math.max(2, Math.round(cfg.d * 0.1)) : 0
    border.color: cfg.outline ? cell.bg() : "transparent"

    Behavior on color {
      enabled: !Color.isTransitioning
      ColorAnimation {
        duration: Style.animationFast
      }
    }

    Row {
      id: content
      anchors.centerIn: parent
      spacing: Style.marginXXS

      NText {
        anchors.verticalCenter: parent.verticalCenter
        text: cfg.pillLabel(cell.ws, cell.position)
        family: Settings.data.ui.fontFixed
        pointSize: cfg.d * cfg.textRatio
        applyUiScale: false
        font.capitalization: cfg.capitalize ? Font.AllUppercase : Font.MixedCase
        font.weight: cell.active ? Font.Bold : Font.Medium
        color: cfg.outline ? cell.lineText() : cell.fg()
        opacity: cell.active ? 1.0 : 0.7
      }

      // Small gap between the name and the bots.
      Item {
        width: Style.marginS
        height: 1
        visible: cell.instances.length > 0
      }

      // One animated bot per Claude instance running here.
      Repeater {
        model: cell.instances
        delegate: BotIcon {
          anchors.verticalCenter: parent.verticalCenter
          required property var modelData
          status: modelData
          cfg: cell.cfg
          pokeNonce: cell.pokeNonce
        }
      }
    }
  }

  MouseArea {
    anchors.fill: parent
    cursorShape: Qt.PointingHandCursor
    acceptedButtons: Qt.LeftButton
    onClicked: {
      CompositorService.switchToWorkspace(cell.ws);
      if (cell.active)
        cell.poke(); // already-current click; switches fire poke via onActiveChanged
    }
  }
}
