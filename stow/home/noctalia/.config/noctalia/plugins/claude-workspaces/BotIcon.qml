// Animations: one expressive Claude Code bot for a single Claude instance.
// Random emote events (faces + bounce/wiggle), idle breathing, and a forced
// "poke" emote when pokeNonce changes (workspace clicked / switched to).
import QtQuick
import qs.Commons

Item {
  id: bot

  property string status: ""   // "green" | "purple" | "orange"
  property var cfg: null
  property int pokeNonce: 0     // bump to force an emote

  property string faceOverride: ""

  width: cfg.d + 2
  height: cfg.d

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
  }

  Image {
    id: botImg
    anchors.centerIn: parent
    source: bot.faceOverride !== "" ? bot.faceOverride : cfg.statusIcon(bot.status)
    width: cfg.d + 2
    height: cfg.d + 2
    sourceSize.width: Math.round(cfg.d * 2)
    sourceSize.height: Math.round(cfg.d * 2)
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
        to: -3.5
        duration: 120
        easing.type: Easing.OutQuad
      }
      NumberAnimation {
        target: bobT
        property: "y"
        from: -3.5
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
}
