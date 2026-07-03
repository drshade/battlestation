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
  // Agent instances here, keyed by session id (see BarWidget.sidsByWs). The bot
  // Repeater is driven by `sids`; each bot reads its own status/title/kind from
  // the maps, so a status change updates a bot in place without disturbing its
  // siblings' animations.
  property var sids: []
  property var statusBySid: ({})
  property var titleBySid: ({})
  property var kindBySid: ({})
  property var agentsBySid: ({})   // { "<sid>": [{id,type,description,started}] }
  property bool occupied: false
  property bool shown: true
  property int position: 0         // display position (1-based); shown instead of the raw id
  // Slightly dim the active highlight when this pill's MONITOR isn't the
  // focused one: each bar highlights what its display shows (still green),
  // but full brightness marks where the keyboard actually is.
  property bool dimFocus: false

  readonly property bool active: focused
  property int pokeNonce: 0

  function poke() {
    pokeNonce++;
  }
  onActiveChanged: if (active)
    poke()

  // Pill background follows the workspace state; Claude status shows via the bots.
  // (Qt.darker keeps "transparent" transparent, so outline mode is unaffected.)
  function bg() {
    if (active)
      return dimFocus ? Qt.darker(cfg.wsBg("focused"), 1.35) : cfg.wsBg("focused");
    if (occupied)
      return cfg.wsBg("occupied");
    return cfg.wsBg("empty");
  }
  function fg() {
    if (active)
      return dimFocus ? Qt.darker(cfg.wsOn("focused"), 1.2) : cfg.wsOn("focused");
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
        visible: cell.sids.length > 0
      }

      // One "squad" per agent instance here, keyed by session id. The outer model
      // is the sid LIST (identity-stable): a status/title/agent change leaves the
      // list untouched, so the squad isn't recreated -- its bots read new state
      // from the maps and update in place, keeping their breathing/emote timers.
      // Only an instance starting/stopping changes the sequence (and then only this
      // pill's squads rebuild, never another workspace's).
      //
      // A squad = the commander bot + a line of smaller sub-agent bots to its right,
      // one per running subagent. The inner Repeater's model is this sid's agent
      // LIST; BarWidget reuses the list instance across polls unless its content
      // changed, so surviving sub-bots keep their animations between polls (a list
      // change does rebuild this squad's sub-bots, but only this squad's).
      Repeater {
        model: cell.sids
        delegate: Row {
          anchors.verticalCenter: parent.verticalCenter
          required property string modelData          // = the session id
          readonly property var agents: cell.agentsBySid[modelData] || []
          readonly property string botStatus: cell.statusBySid[modelData] || ""
          // Squad-wide: markers carry no kind (lib.rs) — sub-bots inherit
          // the session's, so a codex commander leads codex sub-bots.
          readonly property string botKind: cell.kindBySid[modelData] || "claude"
          spacing: Math.round(cfg.d * 0.12)

          BotIcon {
            anchors.verticalCenter: parent.verticalCenter
            status: botStatus
            title: cell.titleBySid[modelData] || ""
            kind: botKind
            commander: agents.length > 0              // turn to face the squad
            cfg: cell.cfg
            pokeNonce: cell.pokeNonce
          }

          Repeater {
            model: agents
            delegate: BotIcon {
              required property var modelData         // = {id,type,description,started}
              anchors.verticalCenter: parent.verticalCenter
              status: botStatus                       // sub-bots mirror the commander
              kind: botKind                           // inherited from the session
              title: (modelData.type || "agent") + (modelData.description ? " — " + modelData.description : "")
              cfg: cell.cfg
              sizeScale: cfg.subScale
              subordinate: true                       // smaller, pops in
            }
          }
        }
      }
    }
  }
}
