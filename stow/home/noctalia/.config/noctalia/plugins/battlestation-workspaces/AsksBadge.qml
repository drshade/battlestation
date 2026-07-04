// Open-asks badge: the count of asks awaiting the human, colored by the
// queue's highest open urgency (any high -> error, else secondary). Click
// toggles the asks panel (the triage surface). Pure presentation: the data
// arrives through BarWidget's stream subscription and is handed in as plain
// properties, so this stays a dumb capsule like UsageIndicator next door.
import QtQuick
import qs.Commons
import qs.Widgets
import qs.Services.UI

Item {
  id: badge

  property var cfg: null
  property string screenName: ""
  property int count: 0
  property bool anyHigh: false
  property int estMin: 0
  signal activated

  // Collapses to zero width while the queue is empty — an empty queue costs
  // no bar space and no attention (the guardrail: attention features must
  // never cost attention).
  visible: count > 0
  width: visible ? pill.width + Style.marginXS : 0
  height: cfg.barHeight

  function tooltip() {
    return count + (count === 1 ? " ask" : " asks") + (estMin > 0 ? "  ·  ~" + estMin + " min" : "");
  }

  Rectangle {
    id: pill
    anchors.verticalCenter: parent.verticalCenter
    width: Math.max(height, label.implicitWidth + Style.marginS * 2)
    height: cfg.d
    radius: height / 2
    color: badge.anyHigh ? Color.mError : Color.mSecondary

    NText {
      id: label
      anchors.centerIn: parent
      text: badge.count
      pointSize: cfg.d * cfg.textRatio
      applyUiScale: false
      font.weight: Style.fontWeightBold
      color: badge.anyHigh ? Color.mOnError : Color.mOnSecondary
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
