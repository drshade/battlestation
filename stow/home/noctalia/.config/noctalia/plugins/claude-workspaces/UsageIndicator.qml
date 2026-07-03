// Claude plan-usage indicator: the Anthropic sunburst + usage %, with a hover
// tooltip. Polls bsctl usage on its own (cached, ~5 min).
import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Widgets
import qs.Services.UI

Item {
  id: usage

  property var cfg: null
  property string screenName: ""

  property int sessionPct: -1
  property string sessionResets: ""
  property int weeklyPct: -1
  property string weeklyResets: ""

  visible: cfg.showUsage
  width: visible ? usageRow.implicitWidth + Style.marginS : 0
  height: cfg.barHeight

  function tooltip() {
    return "Session  " + sessionPct + "%   ·   resets " + cfg.fmtTime(sessionResets, "HH:mm") + "\n" + "Weekly   " + weeklyPct + "%   ·   resets " + cfg.fmtTime(weeklyResets, "ddd d MMM");
  }

  Timer {
    interval: 300000
    running: cfg.showUsage
    repeat: true
    triggeredOnStart: true
    onTriggered: if (!usageProc.running)
      usageProc.running = true
  }
  Process {
    id: usageProc
    command: ["sh", "-c", "$HOME/.local/bin/bsctl usage"]
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          const u = JSON.parse(text);
          usage.sessionPct = u.sessionPct;
          usage.sessionResets = u.sessionResets;
          usage.weeklyPct = u.weeklyPct;
          usage.weeklyResets = u.weeklyResets;
        } catch (e) {}
      }
    }
  }

  Row {
    id: usageRow
    anchors.centerIn: parent
    spacing: Style.marginXXS

    Image {
      anchors.verticalCenter: parent.verticalCenter
      source: Qt.resolvedUrl("assets/claude.svg")
      width: cfg.d
      height: cfg.d
      sourceSize.width: Math.round(cfg.d * 2)
      sourceSize.height: Math.round(cfg.d * 2)
      fillMode: Image.PreserveAspectFit
      smooth: true
    }
    NText {
      anchors.verticalCenter: parent.verticalCenter
      text: (usage.sessionPct >= 0 ? usage.sessionPct : "—") + "% / " + (usage.weeklyPct >= 0 ? usage.weeklyPct : "—") + "%"
      pointSize: cfg.d * cfg.textRatio
      applyUiScale: false
      color: Color.mOnSurface
    }
  }

  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    onEntered: {
      if (usage.sessionPct >= 0)
        TooltipService.show(usage, usage.tooltip(), BarService.getTooltipDirection(usage.screenName));
    }
    onExited: TooltipService.hide()
    onCanceled: TooltipService.hide()
  }
}
