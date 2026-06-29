// Settings + Configuration layer: resolves plugin settings + the active Noctalia
// theme into the colours, sizes and icon URLs the views consume. One instance is
// created by BarWidget and passed down to the child components.
import QtQuick
import qs.Commons

QtObject {
  id: cfg

  property var pluginApi: null
  property var screen: null

  readonly property var ps: (pluginApi && pluginApi.pluginSettings) ? pluginApi.pluginSettings : ({})
  readonly property string screenName: screen ? screen.name : ""

  // ---- geometry -------------------------------------------------------------
  readonly property real barHeight: Style.getBarHeightForScreen(screenName)
  readonly property real capsuleHeight: Style.getCapsuleHeightForScreen(screenName)
  readonly property real pillSize: 0.7
  readonly property real d: Math.round(capsuleHeight * pillSize)
  readonly property real textRatio: 0.50
  readonly property int characterCount: 20

  // ---- settings -------------------------------------------------------------
  readonly property bool outline: ps.outlinePills === true
  readonly property bool hideTrailing: ps.hideTrailing === true
  readonly property bool showUsage: ps.showUsage === true
  readonly property bool capitalize: ps.capitalizeNames !== false
  readonly property bool override: ps.overrideThemeColors === true

  // animation tunables (Advanced)
  readonly property real breathScale: ps.breathScale || 1.045
  readonly property int breathMs: ps.breathMs || 1700
  readonly property int activeMs: ps.activeMs || 1000  // typical thinking/tool emote gap
  readonly property int waitS: ps.waitS || 30          // typical waiting emote gap (seconds)
  readonly property real jitter: (ps.jitter !== undefined ? ps.jitter : 0.35) // ±randomness applied each cycle

  // ---- colour helpers -------------------------------------------------------
  function contrastOn(hex) {
    const c = Qt.color(hex);
    const lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    return lum > 0.55 ? "#101010" : "#f5f5f5";
  }

  // ---- workspace pill colours (focused / occupied / empty) ------------------
  // none = transparent, grey = mOnSurface@22%, + theme colours.
  function wsKeyColor(key) {
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
      return "transparent"; // none -> no background
    }
  }
  function wsKeyOn(key) {
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
      return Color.mOnSurface; // none / grey
    }
  }
  function wsKeyFor(role) {
    return role === "focused" ? (ps.focusedColor || "primary") : role === "occupied" ? (ps.occupiedColor || "secondary") : (ps.emptyColor || "grey");
  }
  function wsBg(role) {
    if (override) {
      if (role === "focused")
        return ps.focusedCustom || "#5e81ac";
      if (role === "occupied")
        return ps.occupiedCustom || "#434c5e";
      return ps.emptyCustom || "#3a3a3a";
    }
    return wsKeyColor(wsKeyFor(role));
  }
  function wsOn(role) {
    if (override) {
      if (role === "focused")
        return contrastOn(ps.focusedCustom || "#5e81ac");
      if (role === "occupied")
        return contrastOn(ps.occupiedCustom || "#434c5e");
      return contrastOn(ps.emptyCustom || "#3a3a3a");
    }
    return wsKeyOn(wsKeyFor(role));
  }

  // ---- bot icon URLs --------------------------------------------------------
  function statusIcon(status) {
    switch (status) {
    case "green":
      return Qt.resolvedUrl("assets/claudecode-thinking.svg");
    case "purple":
      return Qt.resolvedUrl("assets/claudecode-tool.svg");
    case "orange":
      return Qt.resolvedUrl("assets/claudecode-waiting.svg");
    default:
      return Qt.resolvedUrl("assets/claudecode.svg");
    }
  }
  function statusPrefix(status) {
    return status === "green" ? "claudecode-thinking" : status === "purple" ? "claudecode-tool" : "claudecode-waiting";
  }
  function faceUrl(status, emote) {
    return Qt.resolvedUrl("assets/" + statusPrefix(status) + "-" + emote + ".svg");
  }

  // ---- misc helpers ---------------------------------------------------------
  function pillLabel(ws, pos) {
    if (!ws)
      return "";
    const named = ws.name && String(ws.name).length > 0;
    const n = (pos !== undefined && pos > 0) ? pos : ws.idx;
    return named ? (n + " - " + String(ws.name).substring(0, characterCount)) : String(n);
  }
  function fmtTime(iso, fmt) {
    if (!iso)
      return "?";
    const dt = new Date(iso);
    return isNaN(dt.getTime()) ? "?" : Qt.formatDateTime(dt, fmt);
  }
}
