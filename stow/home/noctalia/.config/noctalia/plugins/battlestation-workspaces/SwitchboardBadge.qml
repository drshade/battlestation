// Switchboard entry point. Unlike the Deck badge it stays present when empty:
// opening an empty Switchboard is how the human creates the first link. The
// compact count is links (standing topology), while unread traffic promotes
// the badge to the attention colour until the recipient collects it.
import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Widgets
import qs.Services.UI

Item {
  id: badge

  property var cfg: null
  property string screenName: ""
  property int namedCount: 0
  property int linkCount: 0
  property int unreadCount: 0
  signal activated

  width: pill.width + Style.marginXS
  height: cfg.barHeight

  function tooltip() {
    var topology = namedCount + (namedCount === 1 ? " named agent" : " named agents")
        + "  ·  " + linkCount + (linkCount === 1 ? " link" : " links");
    return unreadCount > 0 ? topology + "  ·  " + unreadCount + " unread" : topology;
  }

  Rectangle {
    id: pill
    anchors.verticalCenter: parent.verticalCenter
    implicitHeight: badge.cfg.d
    implicitWidth: contents.implicitWidth + Style.marginS * 2
    radius: height / 2
    color: badge.unreadCount > 0 ? Color.mTertiary : Qt.alpha(Color.mPrimary, 0.14)
    border.width: badge.unreadCount > 0 ? 0 : 1
    border.color: Qt.alpha(Color.mPrimary, 0.40)

    RowLayout {
      id: contents
      anchors.centerIn: parent
      spacing: Style.marginXS

      NText {
        text: "⇄"
        pointSize: badge.cfg.d * 0.48
        applyUiScale: false
        font.weight: Style.fontWeightBold
        color: badge.unreadCount > 0 ? Color.mOnTertiary : Color.mPrimary
      }
      NText {
        text: String(badge.linkCount)
        pointSize: badge.cfg.d * badge.cfg.textRatio
        applyUiScale: false
        font.weight: Style.fontWeightBold
        color: badge.unreadCount > 0 ? Color.mOnTertiary : Color.mPrimary
      }
    }
  }

  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: badge.activated()
    onEntered: TooltipService.show(badge, badge.tooltip(), BarService.getTooltipDirection(badge.screenName))
    onExited: TooltipService.hide()
    onCanceled: TooltipService.hide()
  }
}
