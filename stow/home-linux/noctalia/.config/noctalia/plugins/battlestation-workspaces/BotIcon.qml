// Animations: one expressive bot for a single agent instance. Random emote
// events (faces + bounce/wiggle) and a forced "poke" emote when pokeNonce
// changes (workspace clicked / switched to). All motion is TRANSIENT — a
// short burst then idle — deliberately: there is no continuous idle
// animation (breathing was removed as a per-frame CPU cost that repainted
// every bot forever for a pulse too subtle to notice at this size). The
// mechanics here are kind-agnostic; which faces/motions a kind uses (and
// its assets + label) comes from Cfg's kind registry.
//
// Structure: the root Item is STATIC (fixed geometry, no transforms) and
// hosts the hover MouseArea; every visual transform — the commander lean,
// the sub-bot spawn pop, and botImg's own bounce/wiggle — applies to
// `body` or deeper. A hit region that moved with the animations let
// containsMouse oscillate under a stationary cursor, flickering the tooltip.
import QtQuick
import qs.Commons
import qs.Services.UI

Item {
  id: bot

  property string status: ""   // "thinking" | "tooling" | "waiting"
  property var cfg: null
  property int pokeNonce: 0     // bump to force an emote

  property string title: ""        // hover tooltip: session aiTitle, or the sub-bot's "<type> — <description>"
  property string kind: "claude"   // agent kind; presentation resolved via Cfg's kind registry

  // A squad's commander (running subagents) turns to face its line of sub-bots.
  // Sub-bots are `subordinate`: smaller (sizeScale), pop in on spawn.
  property bool commander: false
  property bool subordinate: false
  property real sizeScale: 1.0
  readonly property real d: cfg.d * sizeScale

  property string faceOverride: ""

  // Readable agent name, shown when the session has no title yet.
  function kindLabel() {
    return cfg.kindLabel(kind);
  }

  width: d + 2
  height: d

  // Only a genuine poke (focus switch) emotes -- not the initial binding when a
  // fresh bot is created on an already-focused pill (whose pokeNonce is nonzero).
  property bool pokeReady: false
  onPokeNonceChanged: if (pokeReady)
    performEmote(true)

  // Bots update their status in place (no recreation), so re-pace the emote
  // cadence when crossing the waiting<->active boundary -- otherwise a bot that
  // started out waiting would stay on its slow 30s timer after it got busy (and
  // vice-versa). Only the boundary crossing re-paces, so rapid thinking<->tooling
  // flips during work don't keep restarting (and starving) the timer.
  readonly property bool waiting: status === "waiting"
  onWaitingChanged: {
    emoteTimer.interval = nextDelay();
    emoteTimer.restart();
  }

  function rnd(lo, hi) {
    return lo + Math.random() * (hi - lo);
  }
  // Emote vocabulary per kind+status (weighted by repetition; Cfg's registry).
  function emotePool() {
    return cfg.emotePool(kind, status);
  }
  // Cadence: thinking/tools lively, waiting calm. Each gap is jittered ±cfg.jitter.
  function nextDelay() {
    var base = status === "waiting" ? cfg.waitS * 1000 : cfg.activeMs;
    return base * rnd(1 - cfg.jitter, 1 + cfg.jitter);
  }
  function showFace(emote) {
    bot.faceOverride = cfg.faceUrl(kind, status, emote);
    faceTimer.interval = cfg.faceHoldMs(emote);
    faceTimer.restart();
  }
  function performEmote(force) {
    // One emote at a time, so a face emote (squint/look) never bounces.
    if (!force && (faceTimer.running || bounceAnim.running || wiggleAnim.running))
      return;
    var pool = emotePool();
    var emote = pool[Math.floor(Math.random() * pool.length)];
    // Motions are the two kind-agnostic transforms below; anything else the
    // registry names is a face asset of this kind.
    if (emote === "bounce")
      bounceAnim.restart();
    else if (emote === "wiggle")
      wiggleAnim.restart();
    else
      showFace(emote);
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
    pokeReady = true;
    if (bot.subordinate)
      spawnIn.start();
  }
  // Entrance pop for a freshly-spawned sub-bot. Targets body's scale (a
  // level above botImg's own bounce/wiggle, so they compose) — and the root
  // (with its hover hit region) never scales.
  NumberAnimation {
    id: spawnIn
    target: body
    property: "scale"
    from: 0.2
    to: 1.0
    duration: 300
    easing.type: Easing.OutBack
  }

  // The transformed container: everything visual, nothing hit-tested.
  Item {
    id: body
    anchors.fill: parent
    transformOrigin: Item.Center

    // Commander leans toward its squad (a real "turn"); the lean eases in/out
    // as the subagent count crosses zero. transformOrigin is Center, so layout
    // is unaffected.
    rotation: bot.commander ? 7 : 0
    Behavior on rotation {
      NumberAnimation {
        duration: 280
        easing.type: Easing.OutBack
      }
    }
    // Sub-bots pop in from near-zero when spawned (see spawnIn); commanders stay 1.0.
    scale: bot.subordinate ? 0.2 : 1.0

    Image {
      id: botImg
      anchors.centerIn: parent
      // Commander rests on its kind's `rest` face (watching the squad) between
      // its own transient emotes -- claude's is "look", present for every status
      // (the tool one has star eyes); a kind without one, and every plain bot,
      // rests on the status icon.
      source: bot.faceOverride !== "" ? bot.faceOverride : (bot.commander ? cfg.restIcon(bot.kind, bot.status) : cfg.statusIcon(bot.kind, bot.status))
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
    }
  }

  // Hover -> tooltip: a session bot shows its aiTitle (falling back to the agent
  // name until Claude generates one); a sub-bot shows its "<type> — <description>"
  // (set as `title` by the pill). NoButton so the press still falls through to
  // the pill delegate underneath -- click-to-switch and drag-reorder keep
  // working over the bots. Anchored to the STATIC root, never to `body`: the
  // hit region must not lean or pop with the visuals.
  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    acceptedButtons: Qt.NoButton
    cursorShape: Qt.PointingHandCursor
    onEntered: TooltipService.show(bot, (bot.title && bot.title.length) ? bot.title : bot.kindLabel(), BarService.getTooltipDirection(cfg.screenName))
    onExited: TooltipService.hide()
    onCanceled: TooltipService.hide()
  }
}
