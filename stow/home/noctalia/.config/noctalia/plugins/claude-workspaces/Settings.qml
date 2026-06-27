import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Widgets

ColumnLayout {
  id: root
  property var pluginApi: null
  spacing: Style.marginL

  property bool capitalizeNames: true
  property bool showUsage: false
  property bool overrideThemeColors: false
  property string thinkingColor: "primary"
  property string toolColor: "tertiary"
  property string waitingColor: "error"
  property string noneColor: "none"
  property string thinkingCustom: "#3fb950"
  property string toolCustom: "#a371f7"
  property string waitingCustom: "#c97b47"
  property string noneCustom: "#3a3a3a"
  property bool _loaded: false

  function _load() {
    if (!pluginApi || !pluginApi.pluginSettings)
      return;
    const s = pluginApi.pluginSettings;
    capitalizeNames = s.capitalizeNames !== false;
    showUsage = s.showUsage === true;
    overrideThemeColors = s.overrideThemeColors === true;
    thinkingColor = s.thinkingColor || "primary";
    toolColor = s.toolColor || "tertiary";
    waitingColor = s.waitingColor || "error";
    noneColor = s.noneColor || "none";
    thinkingCustom = s.thinkingCustom || "#3fb950";
    toolCustom = s.toolCustom || "#a371f7";
    waitingCustom = s.waitingCustom || "#c97b47";
    noneCustom = s.noneCustom || "#3a3a3a";
    _loaded = true;
  }

  function saveSettings() {
    if (!pluginApi || !_loaded)
      return;
    const s = pluginApi.pluginSettings;
    s.capitalizeNames = capitalizeNames;
    s.showUsage = showUsage;
    s.overrideThemeColors = overrideThemeColors;
    s.thinkingColor = thinkingColor;
    s.toolColor = toolColor;
    s.waitingColor = waitingColor;
    s.noneColor = noneColor;
    s.thinkingCustom = thinkingCustom;
    s.toolCustom = toolCustom;
    s.waitingCustom = waitingCustom;
    s.noneCustom = noneCustom;
    pluginApi.saveSettings(); // persist to disk + retrigger widget bindings
  }

  onPluginApiChanged: _load()
  Component.onCompleted: _load()

  NToggle {
    Layout.fillWidth: true
    label: "Capitalise workspace names"
    checked: root.capitalizeNames
    onToggled: checked => {
      root.capitalizeNames = checked;
      root.saveSettings();
    }
  }

  NToggle {
    Layout.fillWidth: true
    label: "Show Claude usage"
    description: "Small indicator (left of the workspaces) of plan usage %; hover for details."
    checked: root.showUsage
    onToggled: checked => {
      root.showUsage = checked;
      root.saveSettings();
    }
  }

  NToggle {
    Layout.fillWidth: true
    label: "Override theme colours"
    description: "Pick custom colours instead of following the Noctalia theme."
    checked: root.overrideThemeColors
    onToggled: checked => {
      root.overrideThemeColors = checked;
      root.saveSettings();
    }
  }

  // Theme-key pickers (when following the theme)
  ColumnLayout {
    Layout.fillWidth: true
    visible: !root.overrideThemeColors
    spacing: Style.marginL

    NColorChoice {
      label: "Thinking / processing"
      currentKey: root.thinkingColor
      onSelected: key => {
        root.thinkingColor = key;
        root.saveSettings();
      }
    }
    NColorChoice {
      label: "Running a tool"
      currentKey: root.toolColor
      onSelected: key => {
        root.toolColor = key;
        root.saveSettings();
      }
    }
    NColorChoice {
      label: "Waiting for input"
      currentKey: root.waitingColor
      onSelected: key => {
        root.waitingColor = key;
        root.saveSettings();
      }
    }
    NColorChoice {
      label: "No Claude running"
      currentKey: root.noneColor
      onSelected: key => {
        root.noneColor = key;
        root.saveSettings();
      }
    }
  }

  // Custom colour pickers (when overriding)
  ColumnLayout {
    Layout.fillWidth: true
    visible: root.overrideThemeColors
    spacing: Style.marginM

    NText {
      text: "Thinking / processing"
    }
    NColorPicker {
      selectedColor: root.thinkingCustom
      onColorSelected: c => {
        root.thinkingCustom = c.toString();
        root.saveSettings();
      }
    }
    NText {
      text: "Running a tool"
    }
    NColorPicker {
      selectedColor: root.toolCustom
      onColorSelected: c => {
        root.toolCustom = c.toString();
        root.saveSettings();
      }
    }
    NText {
      text: "Waiting for input"
    }
    NColorPicker {
      selectedColor: root.waitingCustom
      onColorSelected: c => {
        root.waitingCustom = c.toString();
        root.saveSettings();
      }
    }
    NText {
      text: "No Claude running"
    }
    NColorPicker {
      selectedColor: root.noneCustom
      onColorSelected: c => {
        root.noneCustom = c.toString();
        root.saveSettings();
      }
    }
  }
}
