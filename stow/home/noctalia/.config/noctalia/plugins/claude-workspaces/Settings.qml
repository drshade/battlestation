import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Widgets

ColumnLayout {
  id: root
  property var pluginApi: null
  spacing: Style.marginL

  property string displayMode: "pill"
  property bool capitalizeNames: true
  property bool showUsage: false
  property bool customColours: false

  // Claude status colours (used to paint pills in pill mode)
  property string thinkingColor: "primary"
  property string toolColor: "tertiary"
  property string waitingColor: "error"
  property string thinkingCustom: "#3fb950"
  property string toolCustom: "#a371f7"
  property string waitingCustom: "#c97b47"

  // Workspace colours (pill background in icon mode + non-Claude pills)
  property string focusedColor: "primary"
  property string occupiedColor: "secondary"
  property string emptyColor: "none"
  property string focusedCustom: "#5e81ac"
  property string occupiedCustom: "#434c5e"
  property string emptyCustom: "#3a3a3a"

  property bool _loaded: false

  function _load() {
    if (!pluginApi || !pluginApi.pluginSettings)
      return;
    const s = pluginApi.pluginSettings;
    displayMode = s.displayMode || "pill";
    capitalizeNames = s.capitalizeNames !== false;
    showUsage = s.showUsage === true;
    customColours = s.overrideThemeColors === true;
    thinkingColor = s.thinkingColor || "primary";
    toolColor = s.toolColor || "tertiary";
    waitingColor = s.waitingColor || "error";
    thinkingCustom = s.thinkingCustom || "#3fb950";
    toolCustom = s.toolCustom || "#a371f7";
    waitingCustom = s.waitingCustom || "#c97b47";
    focusedColor = s.focusedColor || "primary";
    occupiedColor = s.occupiedColor || "secondary";
    emptyColor = s.emptyColor || "none";
    focusedCustom = s.focusedCustom || "#5e81ac";
    occupiedCustom = s.occupiedCustom || "#434c5e";
    emptyCustom = s.emptyCustom || "#3a3a3a";
    _loaded = true;
  }

  function saveSettings() {
    if (!pluginApi || !_loaded)
      return;
    const s = pluginApi.pluginSettings;
    s.displayMode = displayMode;
    s.capitalizeNames = capitalizeNames;
    s.showUsage = showUsage;
    s.overrideThemeColors = customColours;
    s.thinkingColor = thinkingColor;
    s.toolColor = toolColor;
    s.waitingColor = waitingColor;
    s.thinkingCustom = thinkingCustom;
    s.toolCustom = toolCustom;
    s.waitingCustom = waitingCustom;
    s.focusedColor = focusedColor;
    s.occupiedColor = occupiedColor;
    s.emptyColor = emptyColor;
    s.focusedCustom = focusedCustom;
    s.occupiedCustom = occupiedCustom;
    s.emptyCustom = emptyCustom;
    pluginApi.saveSettings();
  }

  onPluginApiChanged: _load()
  Component.onCompleted: _load()

  NComboBox {
    Layout.fillWidth: true
    label: "Display mode"
    model: [
      {
        "key": "pill",
        "name": "Pill background"
      },
      {
        "key": "icons",
        "name": "Instance icons"
      }
    ]
    currentKey: root.displayMode
    onSelected: key => {
      root.displayMode = key;
      root.saveSettings();
    }
  }

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
    description: "Plan usage % to the left of the workspaces; hover for details."
    checked: root.showUsage
    onToggled: checked => {
      root.showUsage = checked;
      root.saveSettings();
    }
  }

  NToggle {
    Layout.fillWidth: true
    label: "Custom colours"
    description: "Pick exact colours instead of following the Noctalia theme."
    checked: root.customColours
    onToggled: checked => {
      root.customColours = checked;
      root.saveSettings();
    }
  }

  // ---- Claude status colours (pill mode only — icons use fixed logos) -------
  ColumnLayout {
    Layout.fillWidth: true
    visible: root.displayMode === "pill"
    spacing: Style.marginM

    NText {
      text: "Claude status colours"
    }

    ColumnLayout {
      Layout.fillWidth: true
      visible: !root.customColours
      spacing: Style.marginM
      NColorChoice {
        label: "Thinking"
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
    }

    ColumnLayout {
      Layout.fillWidth: true
      visible: root.customColours
      spacing: Style.marginS
      NText {
        text: "Thinking"
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
    }
  }

  // ---- Workspace colours (pill background: icon mode + non-Claude pills) -----
  ColumnLayout {
    Layout.fillWidth: true
    spacing: Style.marginM

    NText {
      text: "Workspace colours"
    }

    ColumnLayout {
      Layout.fillWidth: true
      visible: !root.customColours
      spacing: Style.marginM
      NColorChoice {
        label: "Focused"
        currentKey: root.focusedColor
        onSelected: key => {
          root.focusedColor = key;
          root.saveSettings();
        }
      }
      NColorChoice {
        label: "Occupied"
        currentKey: root.occupiedColor
        onSelected: key => {
          root.occupiedColor = key;
          root.saveSettings();
        }
      }
      NColorChoice {
        label: "Empty"
        currentKey: root.emptyColor
        onSelected: key => {
          root.emptyColor = key;
          root.saveSettings();
        }
      }
    }

    ColumnLayout {
      Layout.fillWidth: true
      visible: root.customColours
      spacing: Style.marginS
      NText {
        text: "Focused"
      }
      NColorPicker {
        selectedColor: root.focusedCustom
        onColorSelected: c => {
          root.focusedCustom = c.toString();
          root.saveSettings();
        }
      }
      NText {
        text: "Occupied"
      }
      NColorPicker {
        selectedColor: root.occupiedCustom
        onColorSelected: c => {
          root.occupiedCustom = c.toString();
          root.saveSettings();
        }
      }
      NText {
        text: "Empty"
      }
      NColorPicker {
        selectedColor: root.emptyCustom
        onColorSelected: c => {
          root.emptyCustom = c.toString();
          root.saveSettings();
        }
      }
    }
  }
}
