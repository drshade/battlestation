import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Widgets
import qs.Services.UI

ColumnLayout {
  id: root
  property var pluginApi: null
  spacing: Style.marginL

  property bool capitalizeNames: true
  property bool showUsage: false
  property bool customColours: false
  property bool outlinePills: false
  property bool hideTrailing: true

  // Animation tunables (Advanced section)
  property real breathScale: 1.045
  property int breathMs: 1700
  property int activeMs: 1000
  property int waitS: 30
  property real jitter: 0.35

  // Workspace colours (pill background / outline)
  property string focusedColor: "primary"
  property string occupiedColor: "secondary"
  property string emptyColor: "grey"
  property string focusedCustom: "#5e81ac"
  property string occupiedCustom: "#434c5e"
  property string emptyCustom: "#3a3a3a"

  property bool _loaded: false

  // Workspace colour options: None (transparent) + Surface (grey) + theme colours.
  readonly property var wsColorModel: [
    {
      "key": "none",
      "name": "None"
    },
    {
      "key": "grey",
      "name": "Grey"
    },
    {
      "key": "primary",
      "name": "Primary"
    },
    {
      "key": "secondary",
      "name": "Secondary"
    },
    {
      "key": "tertiary",
      "name": "Tertiary"
    },
    {
      "key": "error",
      "name": "Error"
    }
  ]
  function wsSwatch(key) {
    switch (key) {
    case "grey":
      return Qt.alpha(Color.mOnSurface, 0.22);
    case "primary":
      return Color.mPrimary;
    case "secondary":
      return Color.mSecondary;
    case "tertiary":
      return Color.mTertiary;
    case "error":
      return Color.mError;
    default:
      return "transparent";
    }
  }
  function wsSwatchOn(key) {
    switch (key) {
    case "primary":
      return Color.mOnPrimary;
    case "secondary":
      return Color.mOnSecondary;
    case "tertiary":
      return Color.mOnTertiary;
    case "error":
      return Color.mOnError;
    default:
      return Color.mOnSurface;
    }
  }

  // Like NColorChoice, but with our None(transparent)+Surface(grey) model.
  component WsColorChoice: RowLayout {
    id: choice
    property string label: ""
    property string currentKey: "none"
    signal selected(string key)
    readonly property int diameter: Style.baseWidgetSize * 0.9 * Style.uiScaleRatio
    Layout.fillWidth: true

    NLabel {
      label: choice.label
    }
    RowLayout {
      Repeater {
        model: root.wsColorModel
        Rectangle {
          property bool isSelected: choice.currentKey === modelData.key
          property bool isHovered: swatchMA.containsMouse
          Layout.alignment: Qt.AlignHCenter
          implicitWidth: choice.diameter
          implicitHeight: choice.diameter
          radius: choice.diameter * 0.5
          color: root.wsSwatch(modelData.key)
          border.color: (isSelected || isHovered) ? Color.mOnSurface : Color.mOutline
          border.width: Style.borderM
          MouseArea {
            id: swatchMA
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onEntered: TooltipService.show(parent, modelData.name)
            onExited: TooltipService.hide()
            onClicked: {
              choice.currentKey = modelData.key;
              choice.selected(modelData.key);
            }
          }
          NIcon {
            anchors.centerIn: parent
            icon: "check"
            pointSize: Math.max(Style.fontSizeXS, parent.width * 0.4)
            color: root.wsSwatchOn(modelData.key)
            font.weight: Style.fontWeightBold
            visible: parent.isSelected
          }
        }
      }
    }
  }

  function _load() {
    if (!pluginApi || !pluginApi.pluginSettings)
      return;
    const s = pluginApi.pluginSettings;
    capitalizeNames = s.capitalizeNames !== false;
    showUsage = s.showUsage === true;
    customColours = s.overrideThemeColors === true;
    outlinePills = s.outlinePills === true;
    hideTrailing = s.hideTrailing !== false;
    breathScale = s.breathScale || 1.045;
    breathMs = s.breathMs || 1700;
    activeMs = s.activeMs || 1000;
    waitS = s.waitS || 30;
    jitter = (s.jitter !== undefined ? s.jitter : 0.35);
    focusedColor = s.focusedColor || "primary";
    occupiedColor = s.occupiedColor || "secondary";
    emptyColor = s.emptyColor || "grey";
    focusedCustom = s.focusedCustom || "#5e81ac";
    occupiedCustom = s.occupiedCustom || "#434c5e";
    emptyCustom = s.emptyCustom || "#3a3a3a";
    _loaded = true;
  }

  function saveSettings() {
    if (!pluginApi || !_loaded)
      return;
    const s = pluginApi.pluginSettings;
    s.capitalizeNames = capitalizeNames;
    s.showUsage = showUsage;
    s.overrideThemeColors = customColours;
    s.outlinePills = outlinePills;
    s.hideTrailing = hideTrailing;
    s.breathScale = breathScale;
    s.breathMs = breathMs;
    s.activeMs = activeMs;
    s.waitS = waitS;
    s.jitter = jitter;
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

  NToggle {
    Layout.fillWidth: true
    label: "Outlined pills"
    description: "Draw the pill colour as an outline with a transparent fill."
    checked: root.outlinePills
    onToggled: checked => {
      root.outlinePills = checked;
      root.saveSettings();
    }
  }

  NToggle {
    Layout.fillWidth: true
    label: "Hide trailing empty workspaces"
    description: "Only show workspaces up to the highest occupied (or focused) one."
    checked: root.hideTrailing
    onToggled: checked => {
      root.hideTrailing = checked;
      root.saveSettings();
    }
  }

  // ---- Workspace colours (pill background / outline) -----
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
      WsColorChoice {
        label: "Focused"
        currentKey: root.focusedColor
        onSelected: key => {
          root.focusedColor = key;
          root.saveSettings();
        }
      }
      WsColorChoice {
        label: "Occupied"
        currentKey: root.occupiedColor
        onSelected: key => {
          root.occupiedColor = key;
          root.saveSettings();
        }
      }
      WsColorChoice {
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

  NCollapsible {
    Layout.fillWidth: true
    label: "Advanced"
    description: "Fine-tune the bot animations."

    NValueSlider {
      Layout.fillWidth: true
      label: "Breathing amount"
      text: ((root.breathScale - 1) * 100).toFixed(1) + "%"
      from: 1.0
      to: 1.12
      stepSize: 0.005
      value: root.breathScale
      defaultValue: 1.045
      showReset: true
      onMoved: value => {
        root.breathScale = value;
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Breathing speed"
      text: (root.breathMs / 1000).toFixed(1) + "s"
      from: 600
      to: 3000
      stepSize: 100
      value: root.breathMs
      defaultValue: 1700
      showReset: true
      onMoved: value => {
        root.breathMs = Math.round(value);
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Active emote gap"
      text: (root.activeMs / 1000).toFixed(1) + "s"
      from: 300
      to: 3000
      stepSize: 100
      value: root.activeMs
      defaultValue: 1000
      showReset: true
      onMoved: value => {
        root.activeMs = Math.round(value);
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Waiting emote gap"
      text: root.waitS + "s"
      from: 5
      to: 120
      stepSize: 5
      value: root.waitS
      defaultValue: 30
      showReset: true
      onMoved: value => {
        root.waitS = Math.round(value);
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Randomness"
      text: Math.round(root.jitter * 100) + "%"
      from: 0
      to: 0.6
      stepSize: 0.05
      value: root.jitter
      defaultValue: 0.35
      showReset: true
      onMoved: value => {
        root.jitter = value;
        root.saveSettings();
      }
    }

    NButton {
      Layout.fillWidth: true
      text: "Reset to defaults"
      icon: "refresh"
      onClicked: {
        root.breathScale = 1.045;
        root.breathMs = 1700;
        root.activeMs = 1000;
        root.waitS = 30;
        root.jitter = 0.35;
        root.saveSettings();
      }
    }
  }
}
