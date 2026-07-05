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
  readonly property int activeMs: ps.activeMs || 1000  // typical thinking/tool emote gap
  readonly property int waitS: ps.waitS || 30          // typical waiting emote gap (seconds)
  readonly property real jitter: (ps.jitter !== undefined ? ps.jitter : 0.35) // ±randomness on each emote-timer gap
  readonly property real subScale: ps.subScale || 0.6  // sub-agent bot size vs the commander

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

  // ---- per-kind (harness) presentation registry -------------------------------
  // THE single point of definition for how an agent kind looks. Adding a new
  // harness (gemini, opencode, ...) = one entry here + an assets/<dir>/ holding
  // the files the entry names -- nothing else changes: every consumer
  // (statusIcon/faceUrl/restIcon, BotIcon's emote pool and tooltip label, the
  // usage icon) resolves through kindDef(). An unknown kind falls back to the
  // claude entry, so a harness with no assets yet renders as a Claude bot
  // instead of breaking.
  //
  // Per entry:
  //   label    tooltip name shown while a session has no title
  //   dir      assets/<dir>/: icon.svg (brand mark), <status>.svg per status,
  //            and <status>-<face>.svg for every face named below
  //   neutral  file stem rendered for an unknown/empty status
  //   rest     face a commander (bot with subagents) rests on between emotes,
  //            watching its squad; needs <status>-<rest>.svg for EVERY status.
  //            "" = no such face, rest on the plain status icon instead.
  //   faces    per status: face-swap emotes the random emote picker may draw
  //   motions  per status: transform emotes (bounce/wiggle -- kind-agnostic
  //            mechanics living in BotIcon) mixed into the same pool
  // A status's pool = faces[status] ++ motions[status]; repeats are weights.
  readonly property var kinds: ({
                                  "claude": {
                                    "label": "Claude Code",
                                    "dir": "claude",
                                    "neutral": "base",
                                    "rest": "look",
                                    "faces": {
                                      "thinking": ["squint", "look", "happy", "blink"],
                                      "tooling": ["surprised", "blink"],
                                      "waiting": ["blink", "sleepy", "look"]
                                    },
                                    "motions": {
                                      "thinking": ["bounce", "bounce"],
                                      "tooling": ["wiggle", "wiggle", "bounce"],
                                      "waiting": ["bounce"]
                                    }
                                  },
                                  "codex": {
                                    "label": "Codex",
                                    "dir": "codex",
                                    "neutral": "waiting",
                                    "rest": "",
                                    "faces": {
                                      "thinking": ["spark", "spark"],
                                      "tooling": ["bracket", "spark"],
                                      "waiting": ["pause", "pause"]
                                    },
                                    "motions": {
                                      "thinking": ["bounce"],
                                      "tooling": ["wiggle", "wiggle", "bounce"],
                                      "waiting": ["bounce"]
                                    }
                                  },
                                  "agy": {
                                    "label": "Antigravity",
                                    "dir": "agy",
                                    "neutral": "waiting",
                                    "rest": "",
                                    "faces": {
                                      "thinking": ["drift", "drift"],
                                      "tooling": ["orbit", "spark"],
                                      "waiting": ["float", "float"]
                                    },
                                    "motions": {
                                      "thinking": ["bounce"],
                                      "tooling": ["wiggle", "wiggle", "bounce"],
                                      "waiting": ["bounce"]
                                    }
                                  }
                                })
  function kindDef(kind) {
    return kinds[kind] || kinds["claude"];
  }
  function kindLabel(kind) {
    return kindDef(kind).label;
  }
  function kindIcon(kind) {
    return Qt.resolvedUrl("assets/" + kindDef(kind).dir + "/icon.svg");
  }

  // The protocol's statuses are semantic (thinking/tooling/waiting); mapping a
  // status to its on-disk stem -- the green/purple/orange asset families -- is
  // this table's job, and every status-string consumer routes through it.
  readonly property var statusStem: ({
                                       "thinking": "thinking", // green
                                       "tooling": "tool",      // purple
                                       "waiting": "waiting"    // orange
                                     })
  // How long each face emote holds before the bot returns to its status icon.
  // Keyed by face name across all kinds (blink is a flicker, sleepy/pause a yawn).
  readonly property var faceHold: ({
                                     "blink": 150,
                                     "squint": 700,
                                     "look": 700,
                                     "happy": 700,
                                     "surprised": 600,
                                     "sleepy": 1200,
                                     "spark": 600,
                                     "bracket": 700,
                                     "pause": 1200,
                                     "drift": 700,
                                     "orbit": 700,
                                     "float": 1200
                                   })
  function faceHoldMs(emote) {
    return faceHold[emote] || 700;
  }
  function statusIcon(kind, status) {
    var def = kindDef(kind);
    return Qt.resolvedUrl("assets/" + def.dir + "/" + (statusStem[status] || def.neutral) + ".svg");
  }
  function faceUrl(kind, status, emote) {
    // Unknown status wears the calm (waiting) family, like statusIcon's neutral.
    return Qt.resolvedUrl("assets/" + kindDef(kind).dir + "/" + (statusStem[status] || "waiting") + "-" + emote + ".svg");
  }
  // Commander rest face (or the plain status icon for kinds without one).
  function restIcon(kind, status) {
    var def = kindDef(kind);
    return def.rest !== "" ? faceUrl(kind, status, def.rest) : statusIcon(kind, status);
  }
  // Weighted emote vocabulary for one kind+status (see the registry above).
  function emotePool(kind, status) {
    var def = kindDef(kind);
    var key = statusStem[status] ? status : "waiting";
    return def.faces[key].concat(def.motions[key]);
  }

  // ---- misc helpers ---------------------------------------------------------
  function pillLabel(name, pos) {
    const named = name && String(name).length > 0;
    const n = (pos !== undefined && pos > 0) ? pos : "?";
    return named ? (n + " - " + String(name).substring(0, characterCount)) : String(n);
  }
  function fmtTime(iso, fmt) {
    if (!iso)
      return "?";
    const dt = new Date(iso);
    return isNaN(dt.getTime()) ? "?" : Qt.formatDateTime(dt, fmt);
  }
}
