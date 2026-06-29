// One workspace pill: the capsule (filled or outlined), the "<n> - <name>"
// label, and a row of animated BotIcons (one per Claude instance in icons mode).
// Purely visual + the active/poke state; drag, drop and click handling live in
// the BarWidget ListView delegate that wraps this.
import QtQuick
import qs.Commons
import qs.Widgets
import qs.Services.Compositor

Item {
  id: cell

  // Keyed by id with live values pushed in by the BarWidget delegate -- the pill
  // never holds a compositor snapshot, so it isn't recreated on compositor churn.
  property int wsId: 0
  property string wsName: ""
  property bool focused: false
  property var cfg: null
  property var instances: []       // statuses of Claude instances here
  property bool occupied: false
  property bool shown: true
  property int position: 0         // display position (1-based); shown instead of the raw id

  readonly property bool active: focused
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
        text: cfg.pillLabel(cell.wsName, cell.position)
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

      // One animated bot per Claude instance running here. Model is the COUNT,
      // not the status array, so a status change updates a bot's `status` in
      // place instead of rebuilding the Repeater (which would recreate -- and
      // restart the breathing/emotes of -- every sibling bot, including calm
      // waiting ones). Delegates are only created/destroyed when the count changes.
      Repeater {
        model: cell.instances.length
        delegate: BotIcon {
          anchors.verticalCenter: parent.verticalCenter
          required property int index
          status: cell.instances[index] || ""
          cfg: cell.cfg
          pokeNonce: cell.pokeNonce
        }
      }
    }
  }
}
