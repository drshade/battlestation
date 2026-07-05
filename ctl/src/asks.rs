//! `bsctl asks` — the attention queue: agents post asks (questions, review
//! requests, FYIs), the human triages them from one surface. The store is
//! the source of truth and RPCs are ephemeral (contract in lib.rs — the
//! stage-2 MCP fast path may block briefly on a fresh ask, but an ask
//! OUTLIVES any connection: answers land here and are collected later).
//! Two ownership namespaces, never one field: agents own `urgency` and
//! `estimate_min` (via `update`), the human owns `note`, `answer` and the
//! order file. Reply text and completion are DECOUPLED — `reply` drafts
//! without releasing a blocked asker, `complete` is the releasing
//! transition, `answer` composes both. Delivery is TRACKED, not assumed:
//! `delivered_at` stamps only when the answer reaches its asker — the two
//! MCP pickup points (see [`mark_delivered`]) plus [`inbox`]'s turn-start
//! injection. Queue order is FIFO; only the human reorders.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{proto, sys};

/// `<runtime-dir>/battlestation-asks` ([`sys::runtime_dir`] — env, then
/// /run/user/<uid>, then /tmp) — its own dir, a SIBLING of battlestation-ws
/// on purpose: the session scan treats that dir's non-dot files as session
/// records, and the stream engine's trigger filter serves whole
/// directories, so co-tenancy would tangle both. Runtime lifetime is
/// deliberate: asks reference sessions, and neither survives a reboot.
pub fn asks_dir() -> PathBuf {
    sys::runtime_dir().join("battlestation-asks")
}

/// `<dir>/asks.json` — one object `{"next_id": <int>, "asks": [..]}`,
/// written atomically (dot-prefixed temp + rename, so the stream engine's
/// non-dot trigger fires exactly once per landed write).
pub fn store_file() -> PathBuf {
    asks_dir().join("asks.json")
}

/// `<dir>/order` — the HUMAN's queue order: ask ids space-separated with a
/// trailing newline, the map-file format. Only the human writes it (panel
/// drag / `asks order set`); agents never reorder.
pub fn order_file() -> PathBuf {
    asks_dir().join("order")
}

/// Serialize every read-modify-write of the store: blocking exclusive
/// flock on `<dir>/.lock` across load->modify->save (the map/prefs
/// pattern). Readers take no lock — writes land by atomic rename, so a
/// reader sees old or new bytes, never a torn store. Best-effort: if the
/// lock can't be created the write proceeds unlocked (an ask must never be
/// droppable because /run filled up).
fn with_store_lock<T>(f: impl FnOnce() -> T) -> T {
    let lock = asks_dir().join(".lock");
    if let Some(dir) = lock.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let held = fs::File::create(&lock)
        .ok()
        .filter(|l| sys::flock_exclusive(l, false));
    let out = f();
    drop(held);
    out
}

// ---- pure logic (proto.rs-style: deterministic, unit-tested) ---------------

/// Store parse: `(next_id, asks)`. A corrupt/missing store reads as a FRESH
/// one (next_id 1, no asks) — next_id can't be preserved out of unreadable
/// bytes, and the id reuse that follows is harmless: the asks those ids
/// named are gone with the corruption, and the whole store dies at reboot
/// anyway. A parseable store with a missing/nonsense next_id heals to
/// max(id)+1 so ids stay unique against the asks that DID survive.
pub fn parse_store(content: &str) -> (i64, Vec<Value>) {
    let Ok(v) = serde_json::from_str::<Value>(content) else {
        return (1, Vec::new());
    };
    let asks: Vec<Value> = v
        .get("asks")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|r| r.is_object()).cloned().collect())
        .unwrap_or_default();
    let max_id = asks
        .iter()
        .filter_map(|r| r.get("id").and_then(Value::as_i64))
        .max()
        .unwrap_or(0);
    let next = v
        .get("next_id")
        .and_then(Value::as_i64)
        .filter(|n| *n > max_id)
        .unwrap_or(max_id + 1);
    (next, asks)
}

/// The resolved queue — the one order every reader emits. Open asks first:
/// those the human's order file lists (in list order, tokens matching ids
/// TEXTUALLY like the map's), then the rest FIFO by `created` — NOT
/// ascending id; FIFO is the queue's contract, though the two only diverge
/// if ids ever stop being append-ordered. Then answered-but-not-dismissed
/// asks, newest completion FIRST (`answered_at` descending — the done pile
/// reads like an archive: what you just completed sits at the boundary,
/// not buried under every older completion); the order file deliberately
/// does not apply to them (an answered ask is no longer triage). Dismissed
/// asks are excluded everywhere — `get --id` is the one direct-lookup
/// exception.
pub fn resolve_order(order: &str, asks: &[Value]) -> Vec<Value> {
    let created = |r: &Value| -> f64 { r.get("created").and_then(Value::as_f64).unwrap_or(0.0) };
    let state = |r: &Value| -> String { proto::field(r, "state") };
    let mut open: Vec<&Value> = asks.iter().filter(|r| state(r) == "open").collect();
    open.sort_by(|a, b| created(a).total_cmp(&created(b)));
    let toks: Vec<&str> = order.split_whitespace().collect();
    let mut out: Vec<Value> = toks
        .iter()
        .filter_map(|t| {
            open.iter()
                .find(|r| {
                    r.get("id")
                        .and_then(Value::as_i64)
                        .is_some_and(|i| i.to_string() == **t)
                })
                .map(|r| (*r).clone())
        })
        .collect();
    let listed = |r: &Value| -> bool {
        r.get("id")
            .and_then(Value::as_i64)
            .is_some_and(|i| toks.contains(&i.to_string().as_str()))
    };
    out.extend(open.iter().filter(|r| !listed(r)).map(|r| (*r).clone()));
    let mut answered: Vec<&Value> = asks.iter().filter(|r| state(r) == "answered").collect();
    // Newest completion first; a missing answered_at (legacy record) sorts
    // oldest. Ties (same write batch) break FIFO by created.
    let answered_at =
        |r: &Value| -> f64 { r.get("answered_at").and_then(Value::as_f64).unwrap_or(0.0) };
    answered.sort_by(|a, b| {
        answered_at(b)
            .total_cmp(&answered_at(a))
            .then(created(a).total_cmp(&created(b)))
    });
    out.extend(answered.into_iter().cloned());
    out
}

/// Humanized age for the table: floor to the largest whole unit —
/// `45s`, `12m`, `3h`, `2d`. A negative age (clock skew) reads `0s`.
pub fn age(now: f64, created: f64) -> String {
    let s = (now - created).max(0.0) as i64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=172_799 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// EST cell: the agent's estimate of HUMAN minutes, `5m` / "" when unset.
pub fn est(estimate_min: Option<i64>) -> String {
    estimate_min.map(|m| format!("{m}m")).unwrap_or_default()
}

/// The STATE cell humans read: an answered ask whose answer has actually
/// reached its asker renders `delivered` (delivered_at is the
/// discriminator; the JSON `state` stays "answered" — delivery is a fact
/// about an answered ask, not a third state).
pub fn state_label(r: &Value) -> String {
    let state = proto::field(r, "state");
    if state == "answered" && r.get("delivered_at").and_then(Value::as_f64).is_some() {
        return "delivered".to_string();
    }
    state
}

/// The queue table, shared by `asks get` and `status`'s asks section so
/// the two views can't drift. BLOCKING = an agent's ask call is parked on
/// this row right now (the yes/blank dialect — it only matters when set).
pub const ASK_HEADERS: [&str; 11] = [
    "ID", "AGE", "TYPE", "URG", "EST", "WS", "KIND", "TITLE", "NOTE", "STATE", "BLOCKING",
];

/// [`ASK_HEADERS`]'s cells for PUBLISHED rows at time `now` (the `blocking`
/// flag is read from the row — callers go through [`published`]/[`rows_json`]).
pub fn ask_cells(rows: &[Value], now: f64) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| {
            vec![
                proto::field(r, "id"),
                age(now, r.get("created").and_then(Value::as_f64).unwrap_or(0.0)),
                proto::field(r, "type"),
                proto::field(r, "urgency"),
                est(r.get("estimate_min").and_then(Value::as_i64)),
                proto::field(r, "ws"),
                proto::field(r, "kind"),
                proto::field(r, "title"),
                proto::field(r, "note"),
                state_label(r),
                proto::yes(r.get("blocking").and_then(Value::as_bool).unwrap_or(false)),
            ]
        })
        .collect()
}

/// The `get --id` detail view: field-per-line, every field, values empty
/// when unset (options join with " | "). AGE is derived; the raw `created`
/// stays json-only.
pub fn detail_text(r: &Value, now: f64) -> String {
    let options = r
        .get("options")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_default();
    let rows = [
        ("id", proto::field(r, "id")),
        ("state", state_label(r)),
        (
            "blocking",
            proto::yes(r.get("blocking").and_then(Value::as_bool).unwrap_or(false)),
        ),
        ("type", proto::field(r, "type")),
        ("urgency", proto::field(r, "urgency")),
        (
            "estimate",
            est(r.get("estimate_min").and_then(Value::as_i64)),
        ),
        (
            "age",
            age(now, r.get("created").and_then(Value::as_f64).unwrap_or(0.0)),
        ),
        ("kind", proto::field(r, "kind")),
        ("session", proto::field(r, "session")),
        ("ws", proto::field(r, "ws")),
        ("title", proto::field(r, "title")),
        ("body", proto::field(r, "body")),
        ("options", options),
        ("note", proto::field(r, "note")),
        ("answer", proto::field(r, "answer")),
    ];
    let mut out = String::new();
    for (k, v) in rows {
        let line = format!("{k:<9}{v}");
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

// ---- store ops ---------------------------------------------------------------

fn load() -> (i64, Vec<Value>) {
    parse_store(&fs::read_to_string(store_file()).unwrap_or_default())
}

fn save(next_id: i64, asks: &[Value]) -> std::io::Result<()> {
    let dir = asks_dir();
    fs::create_dir_all(&dir)?;
    sys::atomic_write_json(
        &dir,
        "asks.json",
        &json!({"next_id": next_id, "asks": asks}),
    )
}

/// Locked read-modify-write over one ask: find by id, check, mutate, save.
/// `Err(msg)` becomes the verb's stderr + exit 1.
fn modify(verb: &str, id: i64, f: impl FnOnce(&mut Value) -> Result<(), String>) -> i32 {
    with_store_lock(|| {
        let (next, mut asks) = load();
        let Some(r) = asks
            .iter_mut()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
        else {
            eprintln!("bsctl asks {verb}: no ask {id}");
            return 1;
        };
        if let Err(msg) = f(r) {
            eprintln!("bsctl asks {verb}: {msg}");
            return 1;
        }
        match save(next, &asks) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("bsctl asks {verb}: {}: {e}", store_file().display());
                1
            }
        }
    })
}

// ---- verbs ---------------------------------------------------------------------

/// Append one ask and return its id — the write path shared by `asks post`
/// and the MCP server (which owns stdout as its protocol channel and must
/// never have an id printed under it).
#[allow(clippy::too_many_arguments)]
pub fn create(
    ask_type: &str,
    title: &str,
    body: &str,
    options: &[String],
    urgency: &str,
    estimate_min: Option<i64>,
    kind: &str,
    session: &str,
    ws: Option<i64>,
) -> Result<i64, String> {
    with_store_lock(|| {
        let (next, mut asks) = load();
        asks.push(json!({
            "id": next,
            "session": session,
            "kind": kind,
            "ws": ws,
            "type": ask_type,
            "title": title,
            "body": body,
            "options": options,
            "urgency": urgency,
            "estimate_min": estimate_min,
            "note": "",
            "state": "open",
            "answer": Value::Null,
            "created": sys::now_f64(),
            "answered_at": Value::Null,
            "delivered_at": Value::Null,
            "waiting_pid": Value::Null,
        }));
        match save(next + 1, &asks) {
            Ok(()) => Ok(next),
            Err(e) => Err(format!("{}: {e}", store_file().display())),
        }
    })
}

/// `asks post` — append one ask, print its id (`ask 7`). The MCP server is
/// the usual writer (via [`create`]) and fills session/kind/ws from the
/// calling harness; the flags exist for shell agents and tests.
#[allow(clippy::too_many_arguments)]
pub fn post(
    ask_type: &str,
    title: &str,
    body: &str,
    options: &[String],
    urgency: &str,
    estimate_min: Option<i64>,
    kind: &str,
    session: &str,
    ws: Option<i64>,
) -> i32 {
    match create(
        ask_type,
        title,
        body,
        options,
        urgency,
        estimate_min,
        kind,
        session,
        ws,
    ) {
        Ok(id) => {
            println!("ask {id}");
            0
        }
        Err(e) => {
            eprintln!("bsctl asks post: {e}");
            1
        }
    }
}

/// One ask by id, any state — the MCP server's answer-collection read
/// (`get_ask`, and the `ask` fast path's poll). Lock-free like every read.
pub fn record(id: i64) -> Option<Value> {
    let (_, asks) = load();
    asks.into_iter()
        .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
}

/// The resolved queue as its published JSON rows — shared by `asks get
/// --format json`, its stream form, and the `status` world section. Rows
/// pass through [`published`] (blocking computed, waiting_pid stripped).
pub fn rows_json(session: Option<&str>) -> Vec<Value> {
    let (_, asks) = load();
    let order = fs::read_to_string(order_file()).unwrap_or_default();
    resolve_order(&order, &asks)
        .into_iter()
        .filter(|r| session.is_none_or(|s| proto::field(r, "session") == s))
        .map(published)
        .collect()
}

/// One `asks get` text result — the queue table, or "" when empty (no
/// lonely headers). Shared by the one-shot form and its stream framing.
pub fn get_text(session: Option<&str>) -> String {
    let rows = rows_json(session);
    if rows.is_empty() {
        return String::new();
    }
    proto::render_table(&ASK_HEADERS, &ask_cells(&rows, sys::now_f64())) + "\n"
}

/// One `asks get --format json` result as its JSON line.
pub fn get_json(session: Option<&str>) -> String {
    Value::Array(rows_json(session)).to_string()
}

/// `asks get [--id N | --session S]`. `--id` is a DIRECT record lookup
/// (any state, dismissed included — it's the debugging view); the queue
/// forms exclude dismissed asks by the resolve rule.
pub fn get(id: Option<i64>, session: Option<&str>, json_out: bool) -> i32 {
    if let Some(id) = id {
        let (_, asks) = load();
        let Some(r) = asks
            .iter()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
        else {
            eprintln!("bsctl asks get: no ask {id}");
            return 1;
        };
        let r = published(r.clone());
        if json_out {
            println!("{r}");
        } else {
            print!("{}", detail_text(&r, sys::now_f64()));
        }
        return 0;
    }
    if json_out {
        println!("{}", get_json(session));
    } else {
        print!("{}", get_text(session));
    }
    0
}

/// `asks answer <id> <text>..` — the compose shortcut: reply + complete in
/// ONE locked write (not two verbs chained — a reader must never see the
/// text without the state). Open asks only (an answered ask already has
/// one; re-answering would silently clobber what the asker may have
/// collected). For the decoupled forms see [`reply`]/[`complete`]/[`reopen`].
pub fn answer(id: i64, text: &str) -> i32 {
    modify("answer", id, |r| {
        let state = proto::field(r, "state");
        if state != "open" {
            return Err(format!("ask {id} is {state}, not open"));
        }
        r["answer"] = json!(text);
        r["state"] = json!("answered");
        r["answered_at"] = json!(sys::now_f64());
        Ok(())
    })
}

/// `asks reply <id> [<text>..]` — set/update the reply text WITHOUT
/// touching state (empty text clears it). On an open ask this is a DRAFT,
/// and that is the feature's point: the MCP block loop releases on STATE
/// (mcp::block_verdict), so a draft does not release a blocked agent —
/// the human can keep revising while "still working on it" — yet an agent
/// peeking via get_ask sees the reply-in-progress. Also legal on an
/// answered ask (fixing a typo in a completed reply); dismissed asks are
/// out of the conversation.
pub fn reply(id: i64, text: &str) -> i32 {
    modify("reply", id, |r| {
        let state = proto::field(r, "state");
        if state == "dismissed" {
            return Err(format!("ask {id} is dismissed"));
        }
        r["answer"] = if text.is_empty() {
            Value::Null
        } else {
            json!(text)
        };
        Ok(())
    })
}

/// `asks complete <id>` — open -> answered, whatever the reply text says
/// (completing with no text is a legitimate ack: "seen, no comment"). This
/// is the transition that releases a blocked asker.
pub fn complete(id: i64) -> i32 {
    modify("complete", id, |r| {
        let state = proto::field(r, "state");
        if state != "open" {
            return Err(format!("ask {id} is {state}, not open"));
        }
        r["state"] = json!("answered");
        r["answered_at"] = json!(sys::now_f64());
        Ok(())
    })
}

/// `asks reopen <id>` — answered -> open; the reply text is KEPT (it
/// becomes a draft again) and answered_at clears — as does delivered_at:
/// whatever the NEXT completion says is by definition undelivered. One-way
/// hazard, stated honestly: an asker that already collected the answer (a
/// blocking return or get_ask) cannot have it recalled — reopen governs
/// the queue, not the past.
pub fn reopen(id: i64) -> i32 {
    modify("reopen", id, |r| {
        let state = proto::field(r, "state");
        if state != "answered" {
            return Err(format!("ask {id} is {state}, not answered"));
        }
        r["state"] = json!("open");
        r["answered_at"] = Value::Null;
        r["delivered_at"] = Value::Null;
        Ok(())
    })
}

/// `asks wake <id>` — nudge the asking session's terminal to collect an
/// answer. The Deck is a read-only queue: an answer sits until the asker
/// picks it up (its blocking `ask` RPC returns, a Claude/Codex turn-boundary
/// `inbox` injection, or an explicit `get_ask`). A session PARKED IDLE at its
/// prompt starts no turn, so nothing consumes the answer until the human pokes
/// it — this is that poke. It is a TRIGGER, not a delivery: it types a fixed
/// `[Deck] …call get_ask N` line (submitted) so the asker fetches the answer
/// itself, which is the one path every harness has (agy has no turn injection)
/// and which stamps `delivered_at` on its own via [`mark_delivered`] — so wake
/// never carries answer content and never marks delivery. No-op (exit 0) when
/// the ask is `blocking`: the parked RPC returns the answer directly, and
/// there is no prompt to type into. Routing + the idle status gate live in
/// [`crate::agents::send_text`]; a busy or socket-less session is its loud
/// refusal (the answer is already stored — the harness collects it later).
pub fn wake(id: i64) -> i32 {
    let Some(r) = record(id) else {
        eprintln!("bsctl asks: no ask #{id}");
        return 1;
    };
    if blocking(&r) {
        return 0; // parked in the RPC — answering already delivered it
    }
    let session = proto::field(&r, "session");
    if session.is_empty() {
        eprintln!("bsctl asks: ask #{id} has no session to wake");
        return 1;
    }
    let text = format!("[Deck] ask #{id} was answered — call get_ask {id} to read the answer.");
    crate::agents::send_text(&session, text.as_bytes(), true, false)
}

/// Stamp `delivered_at` — the answer actually REACHED its asker. Exactly
/// two callers, both MCP-side (the blocking `ask` return and an
/// own-session `get_ask` collection); CLI reads and the panel never stamp
/// (a human looking is not delivery — contract in lib.rs). Idempotent
/// (first delivery wins) and quiet: only an answered, unstamped ask is
/// touched, and a vanished id is a no-op — delivery marking is
/// best-effort bookkeeping that must never fail the collection itself.
pub fn mark_delivered(id: i64) {
    with_store_lock(|| {
        let (next, mut asks) = load();
        let Some(r) = asks
            .iter_mut()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
        else {
            return;
        };
        if proto::field(r, "state") != "answered" || !r["delivered_at"].is_null() {
            return;
        }
        r["delivered_at"] = json!(sys::now_f64());
        if let Err(e) = save(next, &asks) {
            eprintln!("bsctl asks: mark delivered {id}: {e}");
        }
    })
}

/// Record that an MCP `ask` call is PARKED on this ask right now — the
/// asker's server pid, stamped as the block loop enters and cleared by
/// [`clear_waiting`] on every exit path. Internal bookkeeping (readers
/// publish it as the computed `blocking` flag, never the pid — see
/// [`published`]); quiet like [`mark_delivered`]: liveness bookkeeping must
/// never fail the ask itself. A server that dies mid-block can't clear —
/// that lie is neutralized at read time by [`blocking`]'s pid check.
pub fn mark_waiting(id: i64, pid: u32) {
    set_waiting(id, json!(pid));
}

/// Clear the parked marker (answered/dismissed/timeout/cancel — the block
/// loop's one shared exit).
pub fn clear_waiting(id: i64) {
    set_waiting(id, Value::Null);
}

fn set_waiting(id: i64, pid: Value) {
    with_store_lock(|| {
        let (next, mut asks) = load();
        let Some(r) = asks
            .iter_mut()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
        else {
            return;
        };
        if r.get("waiting_pid") == Some(&pid) {
            return; // no-op writes would wake the stream for nothing
        }
        r["waiting_pid"] = pid;
        if let Err(e) = save(next, &asks) {
            eprintln!("bsctl asks: mark waiting {id}: {e}");
        }
    })
}

/// Is an agent parked on this ask RIGHT NOW? `waiting_pid` set AND that
/// pid alive (`/proc/<pid>` — local by construction, the session sweep's
/// idiom). The liveness check is the read-time sanitize: a server that
/// crashed mid-block leaves a stale pid, and a dead pid must read as
/// not-blocking rather than lie forever.
pub fn blocking(r: &Value) -> bool {
    r.get("waiting_pid")
        .and_then(Value::as_i64)
        .is_some_and(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
}

/// The published form of a record: the computed `blocking` flag in, the
/// internal `waiting_pid` out. Every emitting surface (queue rows, `get
/// --id`, the MCP tools) goes through here; [`record`] stays raw for the
/// internal readers (the block loop's verdict poll, ownership checks).
pub fn published(mut r: Value) -> Value {
    let b = blocking(&r);
    if let Some(o) = r.as_object_mut() {
        o.remove("waiting_pid");
        o.insert("blocking".into(), json!(b));
    }
    r
}

/// `asks dismiss <id>` — from ANY state: dismissing an open ask is the
/// human declining to answer. Unknown id is quiet success (idempotent, the
/// prefs-rm pattern — a dismiss raced by a reboot must not error scripts).
pub fn dismiss(id: i64) -> i32 {
    with_store_lock(|| {
        let (next, mut asks) = load();
        let Some(r) = asks
            .iter_mut()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
        else {
            return 0;
        };
        r["state"] = json!("dismissed");
        match save(next, &asks) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("bsctl asks dismiss: {}: {e}", store_file().display());
                1
            }
        }
    })
}

/// `asks note <id> [<text>..]` — the human's quick-tag ("working on it");
/// empty text clears. Open or answered asks only — a dismissed ask is out
/// of the conversation.
pub fn note(id: i64, text: &str) -> i32 {
    modify("note", id, |r| {
        let state = proto::field(r, "state");
        if state == "dismissed" {
            return Err(format!("ask {id} is dismissed"));
        }
        r["note"] = json!(text);
        Ok(())
    })
}

/// `asks update <id> [--urgency U] [--estimate-min N]` — the agent's side
/// of the split namespaces: urgency and estimate are the ONLY fields an
/// agent may touch after posting (escalation = "my urgency rose"). Open
/// asks only.
pub fn update(id: i64, urgency: Option<&str>, estimate_min: Option<i64>) -> i32 {
    modify("update", id, |r| {
        let state = proto::field(r, "state");
        if state != "open" {
            return Err(format!("ask {id} is {state}, not open"));
        }
        if let Some(u) = urgency {
            r["urgency"] = json!(u);
        }
        if let Some(m) = estimate_min {
            r["estimate_min"] = json!(m);
        }
        Ok(())
    })
}

/// `asks order set <ids>..` — the human's order file, map-set semantics:
/// the full list, no validation (ids that die resolve away like map
/// tokens).
pub fn order_set(ids: &[i64]) -> i32 {
    let p = order_file();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let body = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(" ") + "\n";
    with_store_lock(|| match fs::write(&p, body) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bsctl asks order set: {}: {e}", p.display());
            1
        }
    })
}

/// `asks order get` — raw file bytes, silent if missing.
pub fn order_get() -> i32 {
    if let Ok(b) = fs::read(order_file()) {
        let _ = std::io::stdout().write_all(&b);
    }
    0
}

// ---- inbox (turn-boundary delivery) -------------------------------------------

/// The ownership rule shared by delivery stamping, the MCP `mine` label and
/// the inbox: both identities non-empty AND equal. An unresolved identity
/// (empty session) must never own anything via the empty==empty accident,
/// and a row with no session belongs to nobody.
pub fn owns(session: &str, owner: &str) -> bool {
    !session.is_empty() && !owner.is_empty() && session == owner
}

/// Title cell for an inbox line: chars-safe truncation with an ellipsis
/// (titles are the human's one-liners; the ANSWER is the payload and is
/// never truncated).
fn short_title(title: &str) -> String {
    let mut it = title.chars();
    let cut: String = it.by_ref().take(50).collect();
    if it.next().is_some() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// The context block one inbox delivery injects — pure for the tests.
/// `(id, title, answer)` per delivered ask; a None answer is an ack-only
/// completion and says so.
pub fn inbox_text(delivered: &[(i64, String, Option<String>)]) -> String {
    let mut out = String::new();
    for (id, title, answer) in delivered {
        let what = match answer {
            Some(a) => format!("was answered: \"{a}\""),
            None => "was completed without reply text (an acknowledgment)".to_string(),
        };
        out.push_str(&format!(
            "[asks] Your ask #{id} (\"{}\") {what}\n",
            short_title(title)
        ));
    }
    if !out.is_empty() {
        out.push_str("[asks] (Delivered — no need to re-raise these with the human.)");
    }
    out
}

/// `asks inbox` — turn-boundary delivery, the third pickup point (after
/// the blocking `ask` return and an own-session `get_ask`): wired as a
/// SYNCHRONOUS UserPromptSubmit hook, it hands the session's answered,
/// undelivered asks to the agent as injected context the moment its next
/// turn starts — the agent never has to remember to check. Hook surface,
/// hook contract: payload JSON on stdin (session_id, falling back to agy's
/// conversationId), an optional `--session-id` argv override outranking
/// it, ALWAYS exit 0, and no session resolvable means print nothing.
/// Collect-and-stamp happens under ONE store lock — the printed text IS
/// the delivery, so the stamp rides the same write. Output is the
/// hookSpecificOutput envelope (`additionalContext`) rather than bare
/// text: Claude Code documents both forms for UserPromptSubmit, Codex's
/// hook runtime models exactly this wire shape, and the envelope is the
/// one format both parse (a harness that injected it raw would still be
/// legible).
pub fn inbox(args: &[String]) -> i32 {
    // Tolerant --session-id extraction, agents-set discipline: this is a
    // hook surface, so flag junk is ignored, never errored.
    let mut session = String::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--session-id" {
            session = args.get(i + 1).cloned().unwrap_or_default();
            i += 2;
        } else if let Some(v) = a.strip_prefix("--session-id=") {
            session = v.to_string();
            i += 1;
        } else {
            i += 1;
        }
    }
    let mut input = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut input);
    if session.is_empty() {
        session = proto::session_key(&proto::parse_payload(&input));
    }
    if session.is_empty() {
        return 0; // no identity, no inbox — silently, like every hook path
    }
    let delivered: Vec<(i64, String, Option<String>)> = with_store_lock(|| {
        let (next, mut asks) = load();
        let mut out = Vec::new();
        for r in asks.iter_mut() {
            if proto::field(r, "state") == "answered"
                && r["delivered_at"].is_null()
                && owns(&session, &proto::field(r, "session"))
            {
                out.push((
                    r.get("id").and_then(Value::as_i64).unwrap_or(0),
                    proto::field(r, "title"),
                    r.get("answer").and_then(Value::as_str).map(str::to_string),
                ));
                r["delivered_at"] = json!(sys::now_f64());
            }
        }
        if !out.is_empty() && save(next, &asks).is_err() {
            // The write failing must not deliver-and-forget: an unstamped
            // store means the next inbox re-delivers, which beats losing
            // the answer. Print anyway.
        }
        out
    });
    if delivered.is_empty() {
        return 0;
    }
    println!(
        "{}",
        json!({"hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": inbox_text(&delivered),
        }})
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(id: i64, state: &str, created: f64) -> Value {
        json!({
            "id": id, "session": format!("s-{id}"), "kind": "claude", "ws": 3,
            "type": "question", "title": format!("t{id}"), "body": "",
            "options": [], "urgency": "medium", "estimate_min": Value::Null,
            "note": "", "state": state, "answer": Value::Null,
            "created": created, "answered_at": Value::Null,
            "delivered_at": Value::Null,
        })
    }

    #[test]
    fn inbox_text_formats_answers_and_acks() {
        assert_eq!(inbox_text(&[]), "");
        let one = inbox_text(&[(26, "Overnight report".into(), Some("all working!".into()))]);
        assert_eq!(
            one,
            "[asks] Your ask #26 (\"Overnight report\") was answered: \"all working!\"\n\
             [asks] (Delivered — no need to re-raise these with the human.)"
        );
        // ack-only completions say so; long titles truncate chars-safe
        let long = "x".repeat(60);
        let two = inbox_text(&[(3, long.clone(), None)]);
        assert!(two.contains("was completed without reply text"));
        assert!(two.contains(&format!("(\"{}…\")", "x".repeat(50))));
        // multi-ask: one line each, one trailer
        let both = inbox_text(&[(1, "a".into(), Some("yes".into())), (2, "b".into(), None)]);
        assert_eq!(both.matches("[asks] Your ask").count(), 2);
        assert_eq!(both.matches("no need to re-raise").count(), 1);
    }

    #[test]
    fn ownership_requires_both_sides_non_empty_and_equal() {
        assert!(owns("sess-1", "sess-1"));
        assert!(!owns("sess-1", "sess-2"));
        // the empty==empty accident: an unresolved identity owns NOTHING,
        // and a session-less row belongs to nobody
        assert!(!owns("", ""));
        assert!(!owns("", "sess-1"));
        assert!(!owns("sess-1", ""));
    }

    #[test]
    fn state_label_discriminates_delivery() {
        // delivered only when answered AND stamped
        let mut r = ask(1, "answered", 0.0);
        assert_eq!(state_label(&r), "answered");
        r["delivered_at"] = json!(123.0);
        assert_eq!(state_label(&r), "delivered");
        // other states never read as delivered, stamp or not
        let mut open = ask(2, "open", 0.0);
        open["delivered_at"] = json!(123.0); // impossible by contract, but honest
        assert_eq!(state_label(&open), "open");
        assert_eq!(state_label(&ask(3, "dismissed", 0.0)), "dismissed");
        // a legacy record without the field is just its state
        let mut legacy = ask(4, "answered", 0.0);
        legacy.as_object_mut().unwrap().remove("delivered_at");
        assert_eq!(state_label(&legacy), "answered");
    }

    #[test]
    fn blocking_requires_a_live_pid_and_published_strips_it() {
        // no marker: not blocking
        let r = ask(1, "open", 0.0);
        assert!(!blocking(&r));
        // a LIVE pid (our own — always alive) reads as blocking
        let mut live = ask(2, "open", 0.0);
        live["waiting_pid"] = json!(std::process::id());
        assert!(blocking(&live));
        // a DEAD pid (beyond pid_max, the suite's canonical dead pid) is the
        // crashed-server stale marker: sanitized to false at read time
        let mut dead = ask(3, "open", 0.0);
        dead["waiting_pid"] = json!(4_000_000);
        assert!(!blocking(&dead));
        // published(): blocking computed in, waiting_pid stripped out
        let p = published(live);
        assert_eq!(p["blocking"], true);
        assert!(p.get("waiting_pid").is_none());
        let p = published(dead);
        assert_eq!(p["blocking"], false);
        // legacy record without the field publishes false, no panic
        let mut legacy = ask(4, "open", 0.0);
        legacy.as_object_mut().unwrap().remove("waiting_pid");
        assert_eq!(published(legacy)["blocking"], false);
    }

    #[test]
    fn store_parse_is_tolerant_and_heals_next_id() {
        // fresh / corrupt / non-object all read as a fresh store
        assert_eq!(parse_store(""), (1, vec![]));
        assert_eq!(parse_store("not json"), (1, vec![]));
        assert_eq!(parse_store("[1,2]"), (1, vec![]));
        // a good store round-trips
        let (n, asks) =
            parse_store(&json!({"next_id": 7, "asks": [ask(3, "open", 1.0)]}).to_string());
        assert_eq!(n, 7);
        assert_eq!(asks.len(), 1);
        // a missing/nonsense next_id heals to max(id)+1, never below it
        let (n, _) = parse_store(&json!({"asks": [ask(5, "open", 1.0)]}).to_string());
        assert_eq!(n, 6);
        let (n, _) = parse_store(&json!({"next_id": 2, "asks": [ask(5, "open", 1.0)]}).to_string());
        assert_eq!(n, 6, "next_id below max(id) would reuse a live id");
        // non-object junk rows are dropped, not fatal
        let (_, asks) =
            parse_store(&json!({"next_id": 2, "asks": [1, ask(1, "open", 1.0)]}).to_string());
        assert_eq!(asks.len(), 1);
    }

    #[test]
    fn resolve_orders_open_by_file_then_fifo_then_answered() {
        let with_answered_at = |mut a: Value, at: f64| {
            a["answered_at"] = json!(at);
            a
        };
        let asks = vec![
            ask(1, "open", 10.0),
            // answered later than 6 despite being created earlier: the done
            // pile is newest-completion-first, so 2 precedes 6
            with_answered_at(ask(2, "answered", 5.0), 100.0),
            ask(3, "open", 30.0),
            ask(4, "dismissed", 1.0),
            ask(5, "open", 20.0),
            with_answered_at(ask(6, "answered", 2.0), 50.0),
        ];
        let ids = |rows: &[Value]| -> Vec<i64> {
            rows.iter()
                .filter_map(|r| r.get("id").and_then(Value::as_i64))
                .collect()
        };
        // no order file: open FIFO by created, answered tail newest
        // completion first, no dismissed
        assert_eq!(ids(&resolve_order("", &asks)), vec![1, 5, 3, 2, 6]);
        // the human's order lists open asks first, in list order; unlisted
        // open asks follow FIFO; answered stay in the tail (order file does
        // not apply to them — they are past triage)
        assert_eq!(ids(&resolve_order("3 1", &asks)), vec![3, 1, 5, 2, 6]);
        // dead/dismissed/answered ids in the file resolve away silently
        assert_eq!(ids(&resolve_order("99 4 2 5", &asks)), vec![5, 1, 3, 2, 6]);
        // tokens match textually: "05" is not id 5
        assert_eq!(ids(&resolve_order("05", &asks)), vec![1, 5, 3, 2, 6]);
        // legacy records without answered_at sort oldest in the done pile,
        // ties breaking FIFO by created
        let legacy = vec![
            with_answered_at(ask(7, "answered", 9.0), 40.0),
            ask(8, "answered", 3.0),
            ask(9, "answered", 6.0),
        ];
        assert_eq!(ids(&resolve_order("", &legacy)), vec![7, 8, 9]);
    }

    #[test]
    fn age_floors_to_largest_unit() {
        assert_eq!(age(100.0, 55.0), "45s");
        assert_eq!(age(100.0, 100.0), "0s");
        assert_eq!(age(100.0, 200.0), "0s"); // clock skew never negative
        assert_eq!(age(1000.0, 280.0), "12m");
        assert_eq!(age(4000.0, 340.0), "1h");
        assert_eq!(age(200_000.0, 0.0), "2d");
        assert_eq!(est(Some(5)), "5m");
        assert_eq!(est(None), "");
    }

    #[test]
    fn detail_renders_field_per_line() {
        let mut r = ask(7, "answered", 0.0);
        r["options"] = json!(["ship it", "hold"]);
        r["answer"] = json!("ship it");
        r["estimate_min"] = json!(5);
        // detail_text reads the PUBLISHED shape; an unset blocking renders
        // as the empty cell (the line trims bare, like body/note).
        let text = detail_text(&r, 130.0);
        assert_eq!(
            text,
            "id       7\n\
             state    answered\n\
             blocking\n\
             type     question\n\
             urgency  medium\n\
             estimate 5m\n\
             age      2m\n\
             kind     claude\n\
             session  s-7\n\
             ws       3\n\
             title    t7\n\
             body\n\
             options  ship it | hold\n\
             note\n\
             answer   ship it\n"
        );
    }
}
