//! Top-level CLI contract tests — above all that clap's adoption did not
//! break the hook's silent-tolerance contract: hooks must NEVER error
//! loudly, whatever argv they were (mis)wired with.

use std::process::{Command, Stdio};

fn bsctl(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bsctl"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn hook_stays_silent_tolerant_under_clap() {
    for args in [
        &["hook"][..],                         // no verb at all
        &["hook", "explode"],                  // unknown verb (also kindless)
        &["hook", "--flag-shaped-verb"],       // hyphen junk must not hit clap
        &["hook", "waiting", "extra", "junk"], // kindless verb = no-op, still silent
        // --kind is parsed inside hook::run, NOT by clap (a clap option with
        // a missing value exits 2 loudly) — every malformation, including a
        // missing/valueless/empty --kind (mandatory, so a no-op), must stay
        // on the silent exit-0 path:
        &["hook", "--kind", "codex", "thinking"], // no session_id on stdin -> no-op
        &["hook", "--kind"],                      // valueless --kind
        &["hook", "--kind", "codex"],             // kind but no verb
        &["hook", "--kind=", "waiting"],          // empty kind value
        &["hook", "--kind=codex", "explode"],     // unknown verb after kind
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
    for args in [&[][..], &["bogus"], &["poll", "extra"]] {
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
    for sub in ["hook", "poll", "watch", "ws", "usage", "completions"] {
        assert!(help.contains(sub), "--help must mention {sub}: {help}");
    }
}

#[test]
fn completions_generate_fish() {
    let out = bsctl(&["completions", "fish"]);
    assert_eq!(out.status.code(), Some(0));
    let fish = String::from_utf8_lossy(&out.stdout);
    assert!(fish.contains("complete") && fish.contains("bsctl"));
    // the ws subcommands are completable too
    assert!(fish.contains("movewindow"));
}
