//! Top-level CLI contract tests — above all that clap's adoption did not
//! break the hook endpoint's silent-tolerance contract: `agents set` must
//! NEVER error loudly, whatever argv it was (mis)wired with.

use std::process::{Command, Stdio};

fn bsctl(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bsctl"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn agents_set_stays_silent_tolerant_under_clap() {
    for args in [
        &["agents", "set"][..],                         // no verb at all
        &["agents", "set", "explode"],                  // unknown verb (also kindless)
        &["agents", "set", "--flag-shaped-verb"],       // hyphen junk must not hit clap
        &["agents", "set", "waiting", "extra", "junk"], // kindless verb = no-op, still silent
        // --kind and --session-id are parsed inside agents::set, NOT by clap
        // (a clap option with a missing value exits 2 loudly) — every
        // malformation, including a missing/valueless/empty --kind
        // (mandatory, so a no-op), must stay on the silent exit-0 path:
        &["agents", "set", "--kind", "codex", "thinking"], // no session_id on stdin -> no-op
        &["agents", "set", "--kind"],                      // valueless --kind
        &["agents", "set", "--kind", "codex"],             // kind but no verb
        &["agents", "set", "--kind=", "waiting"],          // empty kind value
        &["agents", "set", "--kind=codex", "explode"],     // unknown verb after kind
        &["agents", "set", "--kind=codex", "waiting", "--session-id"], // valueless override
    ] {
        let out = bsctl(args);
        assert_eq!(out.status.code(), Some(0), "{args:?} must exit 0");
        assert!(
            out.stdout.is_empty() && out.stderr.is_empty(),
            "{args:?} must stay silent"
        );
    }
}

#[test]
fn top_level_usage_errors_exit_2() {
    for args in [
        &[][..],
        &["bogus"],
        &["poll"],                                        // retired: agents get took over
        &["watch"],                                       // retired: --stream took over
        &["usage"],                                       // retired: agents usage took over
        &["agents"],                                      // set|get required
        &["comms"],                                       // get/link/etc required
        &["comms", "link", "--a-workspace", "one"],       // both public identities required
        &["ws"],                                          // subcommand required
        &["ws", "focus"],                                 // a selector is required
        &["ws", "send"],                                  // window|workspace required
        &["display", "set"],                              // dpms|scale required
        &["ws", "focus", "--bs-id", "1", "--ws-id", "2"], // selectors are exclusive
        &["ws", "send", "workspace", "--bs-id", "1"],     // workspace sends take displays only
        &["ws", "prefs", "rm"],                           // a selector or --all is required
        &["display", "set", "dpms", "--display-name", "eDP-1"], // --on|--off required
        &["display", "set", "scale", "--up", "--down"],   // actions are exclusive
    ] {
        let out = bsctl(args);
        assert_eq!(out.status.code(), Some(2), "{args:?} must exit 2");
        assert!(!out.stderr.is_empty(), "{args:?} must explain on stderr");
    }
}

#[test]
fn help_lists_all_subcommands() {
    let out = bsctl(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8_lossy(&out.stdout);
    for sub in ["ws", "display", "agents", "comms", "status", "completions"] {
        assert!(help.contains(sub), "--help must mention {sub}: {help}");
    }
    // plan usage lives under agents now (the top-level verb is retired, but
    // clap's "Usage:" header keeps the word in --help, so assert there):
    let agents_help = bsctl(&["agents", "--help"]);
    assert!(
        String::from_utf8_lossy(&agents_help.stdout).contains("usage"),
        "agents --help must list the usage verb"
    );
    // the retired verbs must be GONE, not hidden
    for gone in ["poll", "watch"] {
        assert!(
            !help.contains(gone),
            "--help must not mention {gone}: {help}"
        );
    }
}

#[test]
fn help_renders_at_every_level() {
    for args in [
        &["ws", "--help"][..],
        &["ws", "focus", "--help"],
        &["ws", "send", "--help"],
        &["ws", "send", "window", "--help"],
        &["ws", "name", "--help"],
        &["ws", "map", "--help"],
        &["ws", "prefs", "--help"],
        &["display", "--help"],
        &["display", "set", "--help"],
        &["display", "set", "scale", "--help"],
        &["agents", "--help"],
        &["agents", "get", "--help"],
        &["comms", "--help"],
        &["comms", "get", "--help"],
        &["comms", "link", "--help"],
        &["comms", "unlink", "--help"],
    ] {
        let out = bsctl(args);
        assert_eq!(out.status.code(), Some(0), "{args:?} must exit 0");
        assert!(!out.stdout.is_empty(), "{args:?} must print help");
    }
}

#[test]
fn completions_generate_fish() {
    let out = bsctl(&["completions", "fish"]);
    assert_eq!(out.status.code(), Some(0));
    let fish = String::from_utf8_lossy(&out.stdout);
    assert!(fish.contains("complete") && fish.contains("bsctl"));
    // the nested ws subcommands are completable too
    assert!(fish.contains("prefs") && fish.contains("reconcile"));
}
