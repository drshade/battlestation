// Claude + Codex plan-usage indicator: icon + session% per kind, with a
// hover tooltip. Polls `bsctl agents usage` on its own (cached, ~5 min);
// the output is indexed by harness kind, and this indicator reads both.
import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Widgets
import qs.Services.UI

Item {
  id: usage

  property var cfg: null
  property string screenName: ""

  property int claudeSessionPct: -1
  property string claudeSessionResets: ""
  property int claudeWeeklyPct: -1
  property string claudeWeeklyResets: ""

  property int codexSessionPct: -1
  property string codexSessionResets: ""
  property int codexWeeklyPct: -1
  property string codexWeeklyResets: ""

  visible: cfg.showUsage
  width: visible ? usageRow.implicitWidth + Style.marginS : 0
  height: cfg.barHeight

  function claudeTooltip() {
    return "Claude\nSession  " + claudeSessionPct + "%   ·   resets " + cfg.fmtTime(claudeSessionResets, "HH:mm") + "\n" + "Weekly   " + claudeWeeklyPct + "%   ·   resets " + cfg.fmtTime(claudeWeeklyResets, "ddd d MMM");
  }
  function codexTooltip() {
    return "Codex\nSession  " + codexSessionPct + "%   ·   resets " + cfg.fmtTime(codexSessionResets, "HH:mm") + "\n" + "Weekly   " + codexWeeklyPct + "%   ·   resets " + cfg.fmtTime(codexWeeklyResets, "ddd d MMM");
  }
  function tooltip() {
    return claudeTooltip() + "\n\n" + codexTooltip();
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
    command: ["sh", "-c", "$HOME/.local/bin/bsctl agents usage --format json"]
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          const j = JSON.parse(text);
          const c = j.claude;
          if (c) {
            usage.claudeSessionPct = c.sessionPct;
            usage.claudeSessionResets = c.sessionResets;
            usage.claudeWeeklyPct = c.weeklyPct;
            usage.claudeWeeklyResets = c.weeklyResets;
          }
          const x = j.codex;
          if (x) {
            usage.codexSessionPct = x.sessionPct;
            usage.codexSessionResets = x.sessionResets;
            usage.codexWeeklyPct = x.weeklyPct;
            usage.codexWeeklyResets = x.weeklyResets;
          }
        } catch (e) {}
      }
    }
  }

  Row {
    id: usageRow
    anchors.centerIn: parent
    spacing: Style.marginS

    // Claude
    Row {
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.marginXXS
      Image {
        anchors.verticalCenter: parent.verticalCenter
        source: usage.cfg.kindIcon("claude")
        width: cfg.d
        height: cfg.d
        sourceSize.width: Math.round(cfg.d * 2)
        sourceSize.height: Math.round(cfg.d * 2)
        fillMode: Image.PreserveAspectFit
        smooth: true
      }
      NText {
        anchors.verticalCenter: parent.verticalCenter
        text: (usage.claudeSessionPct >= 0 ? usage.claudeSessionPct : "—") + "%"
        pointSize: cfg.d * cfg.textRatio
        applyUiScale: false
        color: Color.mOnSurface
      }
    }

    // Codex
    Row {
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.marginXXS
      Image {
        anchors.verticalCenter: parent.verticalCenter
        source: usage.cfg.kindIcon("codex")
        width: cfg.d
        height: cfg.d
        sourceSize.width: Math.round(cfg.d * 2)
        sourceSize.height: Math.round(cfg.d * 2)
        fillMode: Image.PreserveAspectFit
        smooth: true
      }
      NText {
        anchors.verticalCenter: parent.verticalCenter
        text: (usage.codexSessionPct >= 0 ? usage.codexSessionPct : "—") + "%"
        pointSize: cfg.d * cfg.textRatio
        applyUiScale: false
        color: Color.mOnSurface
      }
    }
  }

  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    onEntered: {
      if (usage.claudeSessionPct >= 0 || usage.codexSessionPct >= 0)
        TooltipService.show(usage, usage.tooltip(), BarService.getTooltipDirection(usage.screenName));
    }
    onExited: TooltipService.hide()
    onCanceled: TooltipService.hide()
  }
}
