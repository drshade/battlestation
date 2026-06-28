import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Widgets
import qs.Services.UI

ColumnLayout {
  id: root
  property var pluginApi: null
  spacing: Style.marginL

  property string displayMode: "pill"
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
    displayMode = s.displayMode || "pill";
    capitalizeNames = s.capitalizeNames !== false;
    showUsage = s.showUsage === true;
    customColours = s.overrideThemeColors === true;
    outlinePills = s.outlinePills === true;
    hideTrailing = s.hideTrailing !== false;
    breathScale = s.breathScale || 1.045;
    breathMs = s.breathMs || 1700;
    activeMs = s.activeMs || 1000;
    waitS = s.waitS || 30;
    thinkingColor = s.thinkingColor || "primary";
    toolColor = s.toolColor || "tertiary";
    waitingColor = s.waitingColor || "error";
    thinkingCustom = s.thinkingCustom || "#3fb950";
    toolCustom = s.toolCustom || "#a371f7";
    waitingCustom = s.waitingCustom || "#c97b47";
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
    s.displayMode = displayMode;
    s.capitalizeNames = capitalizeNames;
    s.showUsage = showUsage;
    s.overrideThemeColors = customColours;
    s.outlinePills = outlinePills;
    s.hideTrailing = hideTrailing;
    s.breathScale = breathScale;
    s.breathMs = breathMs;
    s.activeMs = activeMs;
    s.waitS = waitS;
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
      from: 1.0
      to: 1.12
      stepSize: 0.005
      value: root.breathScale
      onMoved: value => {
        root.breathScale = value;
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Breathing speed (ms)"
      from: 600
      to: 3000
      stepSize: 100
      value: root.breathMs
      onMoved: value => {
        root.breathMs = Math.round(value);
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Active emote gap (ms)"
      from: 300
      to: 3000
      stepSize: 100
      value: root.activeMs
      onMoved: value => {
        root.activeMs = Math.round(value);
        root.saveSettings();
      }
    }
    NValueSlider {
      Layout.fillWidth: true
      label: "Waiting emote gap (s)"
      from: 5
      to: 120
      stepSize: 5
      value: root.waitS
      onMoved: value => {
        root.waitS = Math.round(value);
        root.saveSettings();
      }
    }
  }
}
