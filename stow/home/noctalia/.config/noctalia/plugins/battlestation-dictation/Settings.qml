// Settings: the microphone picker (the reason this surface exists — recon
// found the system-default source recording digital silence while the
// onboard mic was fine), the max-duration backstop, and idle visibility.
//
// Sources come from `pw-dump` (direct argv, no shell), filtered to
// media.class == "Audio/Source". The picker stores node.name — stable
// across reboots, unlike node ids. "" means follow the system default.
import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import qs.Commons
import qs.Widgets

ColumnLayout {
  id: root
  property var pluginApi: null
  spacing: Style.marginL

  property string micNode: ""
  property int maxDurationS: 60
  property bool showWhenIdle: true

  property bool _loaded: false

  // [{name, desc}] parsed from pw-dump.
  property var sources: []

  function _load() {
    if (!pluginApi || !pluginApi.pluginSettings)
      return;
    const s = pluginApi.pluginSettings;
    micNode = s.micNode || "";
    maxDurationS = s.maxDurationS || 60;
    showWhenIdle = s.showWhenIdle !== false;
    _loaded = true;
  }

  function saveSettings() {
    if (!pluginApi || !_loaded)
      return;
    const s = pluginApi.pluginSettings;
    s.micNode = micNode;
    s.maxDurationS = maxDurationS;
    s.showWhenIdle = showWhenIdle;
    pluginApi.saveSettings();
  }

  onPluginApiChanged: _load()
  Component.onCompleted: {
    _load();
    dumpProc.running = true;
  }

  Process {
    id: dumpProc
    command: ["pw-dump"]
    stdout: StdioCollector {
      id: dumpOut
    }
    onExited: (exitCode, exitStatus) => {
      if (exitCode !== 0)
        return;
      var out = [];
      try {
        var arr = JSON.parse(dumpOut.text);
        for (var i = 0; i < arr.length; i++) {
          var props = arr[i].info?.props;
          if (!props || props["media.class"] !== "Audio/Source")
            continue;
          out.push({
            "name": props["node.name"] || "",
            "desc": props["node.description"] || props["node.name"] || "?"
          });
        }
      } catch (e) {
        // leave sources as-is; the picker still offers System default
      }
      root.sources = out;
    }
  }

  // One selectable row of the mic picker.
  component SourceRow: Rectangle {
    id: row
    property string key: ""
    property string label: ""
    readonly property bool isSelected: root.micNode === key
    Layout.fillWidth: true
    implicitHeight: rowLayout.implicitHeight + Style.marginM * 2
    radius: Style.radiusS
    color: rowMA.containsMouse ? Qt.alpha(Color.mOnSurface, 0.08) : "transparent"
    border.color: isSelected ? Color.mPrimary : Color.mOutline
    border.width: Style.borderS

    RowLayout {
      id: rowLayout
      anchors.fill: parent
      anchors.margins: Style.marginM
      spacing: Style.marginM
      NIcon {
        icon: row.isSelected ? "circle-check" : "microphone"
        color: row.isSelected ? Color.mPrimary : Color.mOnSurfaceVariant
      }
      NText {
        Layout.fillWidth: true
        text: row.label
        elide: Text.ElideRight
      }
    }
    MouseArea {
      id: rowMA
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: {
        root.micNode = row.key;
        root.saveSettings();
      }
    }
  }

  ColumnLayout {
    Layout.fillWidth: true
    spacing: Style.marginS

    NText {
      text: "Microphone"
    }
    NText {
      text: "Which source to record from. Test it: a dead default (unplugged interface, gain at zero) records perfect silence."
      color: Color.mOnSurfaceVariant
      wrapMode: Text.WordWrap
      Layout.fillWidth: true
    }

    SourceRow {
      key: ""
      label: "System default"
    }
    Repeater {
      model: root.sources
      SourceRow {
        key: modelData.name
        label: modelData.desc
      }
    }

    NButton {
      text: "Refresh sources"
      icon: "refresh"
      onClicked: dumpProc.running = true
    }
  }

  NValueSlider {
    Layout.fillWidth: true
    label: "Max recording duration"
    description: "The stuck-mic backstop: recording auto-stops after this long."
    text: root.maxDurationS + "s"
    from: 10
    to: 180
    stepSize: 10
    value: root.maxDurationS
    defaultValue: 60
    showReset: true
    onMoved: value => {
      root.maxDurationS = Math.round(value);
      root.saveSettings();
    }
  }

  NToggle {
    Layout.fillWidth: true
    label: "Show when idle"
    description: "Keep the mic glyph in the bar even when nothing is happening."
    checked: root.showWhenIdle
    onToggled: checked => {
      root.showWhenIdle = checked;
      root.saveSettings();
    }
  }
}
