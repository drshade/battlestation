// Animations: one expressive Claude Code bot for a single Claude instance.
// Random emote events (faces + bounce/wiggle), idle breathing, and a forced
// "poke" emote when pokeNonce changes (workspace clicked / switched to).
import QtQuick
import qs.Commons
import qs.Services.UI

Item {
  id: bot

  property string status: ""   // "green" | "purple" | "orange"
  property var cfg: null
  property int pokeNonce: 0     // bump to force an emote

  property string title: ""        // this session's aiTitle (hover tooltip)
  property string kind: "claude"   // agent kind -- future: "codex" | "gemini" | ...

  // A squad's commander (running subagents) turns to face its line of sub-bots.
  // Sub-bots are `subordinate`: smaller (sizeScale), pop in on spawn, no tooltip.
  property bool commander: false
  property bool subordinate: false
  property real sizeScale: 1.0
  readonly property real d: cfg.d * sizeScale

  property string faceOverride: ""

  // Readable agent name, shown when the session has no title yet. The emote
  // vocabulary + assets are still Claude-only (see Cfg); when other kinds land,
  // switch those on `kind` too.
  function kindLabel() {
    if (kind === "codex")
      return "Codex";
    if (kind === "gemini")
      return "Gemini";
    return "Claude Code";
  }

  width: d + 2
  height: d

  // Commander leans toward its squad (a real "turn"); the lean eases in/out as the
  // subagent count crosses zero. transformOrigin is Center, so layout is unaffected.
  rotation: commander ? 7 : 0
  Behavior on rotation {
    NumberAnimation {
      duration: 280
      easing.type: Easing.OutBack
    }
  }
  // Sub-bots pop in from near-zero when spawned (see spawnIn); commanders stay 1.0.
  scale: subordinate ? 0.2 : 1.0

  // Only a genuine poke (focus switch) emotes -- not the initial binding when a
  // fresh bot is created on an already-focused pill (whose pokeNonce is nonzero).
  property bool pokeReady: false
  onPokeNonceChanged: if (pokeReady)
    performEmote(true)

  // Bots update their status in place (no recreation), so re-pace the emote
  // cadence when crossing the waiting<->active boundary -- otherwise a bot that
  // started out waiting would stay on its slow 30s timer after it got busy (and
  // vice-versa). Only the boundary crossing re-paces, so rapid green<->purple
  // flips during work don't keep restarting (and starving) the timer.
  readonly property bool waiting: status === "orange"
  onWaitingChanged: {
    emoteTimer.interval = nextDelay();
    emoteTimer.restart();
  }

  function rnd(lo, hi) {
    return lo + Math.random() * (hi - lo);
  }
  // Emote vocabulary per status (weighted by repetition).
  function emotePool() {
    if (status === "green")
      return ["bounce", "squint", "look", "happy", "blink", "bounce"];
    if (status === "purple")
      return ["wiggle", "wiggle", "surprised", "blink", "bounce"];
    return ["blink", "sleepy", "look", "bounce"];
  }
  // Cadence: thinking/tools lively, waiting calm. Each gap is jittered ±cfg.jitter.
  function nextDelay() {
    var base = status === "orange" ? cfg.waitS * 1000 : cfg.activeMs;
    return base * rnd(1 - cfg.jitter, 1 + cfg.jitter);
  }
  // Run one breath, re-jittering its amplitude + duration; loops via onFinished.
  function breatheOnce() {
    var amp = (cfg.breathScale - 1) * rnd(1 - cfg.jitter, 1 + cfg.jitter);
    var dur = cfg.breathMs * rnd(1 - cfg.jitter, 1 + cfg.jitter);
    breathUp.to = 1.0 + Math.max(0, amp);
    breathDown.from = breathUp.to;
    breathUp.duration = dur;
    breathDown.duration = dur;
    breathAnim.start();
  }
  function showFace(emote, ms) {
    bot.faceOverride = cfg.faceUrl(status, emote);
    faceTimer.interval = ms;
    faceTimer.restart();
  }
  function performEmote(force) {
    // One emote at a time, so a face emote (squint/look) never bounces.
    if (!force && (faceTimer.running || bounceAnim.running || wiggleAnim.running))
      return;
    var pool = emotePool();
    switch (pool[Math.floor(Math.random() * pool.length)]) {
    case "squint":
      showFace("squint", 700);
      break;
    case "look":
      showFace("look", 700);
      break;
    case "happy":
      showFace("happy", 700);
      break;
    case "surprised":
      showFace("surprised", 600);
      break;
    case "sleepy":
      showFace("sleepy", 1200);
      break;
    case "blink":
      showFace("blink", 150);
      break;
    case "bounce":
      bounceAnim.restart();
      break;
    case "wiggle":
      wiggleAnim.restart();
      break;
    }
  }

  Timer {
    id: emoteTimer
    repeat: false
    onTriggered: {
      bot.performEmote(false);
      interval = bot.nextDelay();
      start();
    }
  }
  Timer {
    id: faceTimer
    repeat: false
    onTriggered: bot.faceOverride = ""
  }
  Component.onCompleted: {
    emoteTimer.interval = bot.nextDelay();
    emoteTimer.start();
    breatheOnce();
    pokeReady = true;
    if (bot.subordinate)
      spawnIn.start();
  }
  // Entrance pop for a freshly-spawned sub-bot. Targets the root `scale` (the
  // breath animates botImg.scale), so the two compose instead of fighting.
  NumberAnimation {
    id: spawnIn
    target: bot
    property: "scale"
    from: 0.2
    to: 1.0
    duration: 300
    easing.type: Easing.OutBack
  }

  Image {
    id: botImg
    anchors.centerIn: parent
    // Commander rests on the "look" face (watching the squad) between its own
    // transient emotes; a plain bot rests on its status icon. Each status has a
    // -look asset (the tool one has star eyes), so this holds in every state.
    source: bot.faceOverride !== "" ? bot.faceOverride : (bot.commander ? cfg.faceUrl(bot.status, "look") : cfg.statusIcon(bot.status))
    width: bot.d + 2
    height: bot.d + 2
    sourceSize.width: Math.round(bot.d * 2)
    sourceSize.height: Math.round(bot.d * 2)
    fillMode: Image.PreserveAspectFit
    smooth: true
    asynchronous: false
    transformOrigin: Item.Center
    transform: Translate {
      id: bobT
    }

    // Bounce emote: a quick double hop.
    SequentialAnimation {
      id: bounceAnim
      NumberAnimation {
        target: bobT
        property: "y"
        from: 0
        to: -3.5 * bot.sizeScale
        duration: 120
        easing.type: Easing.OutQuad
      }
      NumberAnimation {
        target: bobT
        property: "y"
        from: -3.5 * bot.sizeScale
        to: 0
        duration: 240
        easing.type: Easing.OutBounce
      }
    }
    // Wiggle emote: a quick shake.
    SequentialAnimation {
      id: wiggleAnim
      NumberAnimation {
        target: botImg
        property: "rotation"
        from: 0
        to: -9
        duration: 60
      }
      NumberAnimation {
        target: botImg
        property: "rotation"
        from: -9
        to: 9
        duration: 100
      }
      NumberAnimation {
        target: botImg
        property: "rotation"
        from: 9
        to: -6
        duration: 90
      }
      NumberAnimation {
        target: botImg
        property: "rotation"
        from: -6
        to: 0
        duration: 70
      }
    }
    // Idle breathing: a slight scale pulse, re-jittered each cycle (see breatheOnce).
    SequentialAnimation {
      id: breathAnim
      NumberAnimation {
        id: breathUp
        target: botImg
        property: "scale"
        from: 1.0
        to: cfg.breathScale
        duration: cfg.breathMs
        easing.type: Easing.InOutSine
      }
      NumberAnimation {
        id: breathDown
        target: botImg
        property: "scale"
        from: cfg.breathScale
        to: 1.0
        duration: cfg.breathMs
        easing.type: Easing.InOutSine
      }
      onFinished: Qt.callLater(bot.breatheOnce)
    }
  }

  // Hover -> tooltip with this instance's session title (falls back to the agent
  // name until Claude generates one). NoButton so the press still falls through
  // to the pill delegate underneath -- click-to-switch and drag-reorder keep
  // working over the bots.
  MouseArea {
    anchors.fill: parent
    enabled: !bot.subordinate          // sub-bots are decorative, not interactive
    hoverEnabled: !bot.subordinate
    acceptedButtons: Qt.NoButton
    cursorShape: Qt.PointingHandCursor
    onEntered: TooltipService.show(bot, (bot.title && bot.title.length) ? bot.title : bot.kindLabel(), BarService.getTooltipDirection(cfg.screenName))
    onExited: TooltipService.hide()
    onCanceled: TooltipService.hide()
  }
}
