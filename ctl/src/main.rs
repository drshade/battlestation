use std::process::exit;

const USAGE: &str = "\
usage: bsctl <subcommand>

  hook <waiting|thinking|tooling|clear|agent-start|agent-stop>
        Claude Code hook endpoint; hook-event JSON on stdin. Writes the
        claude-ws state files. Protocol: claude-ws-status.sh header.
        Silent-tolerant by contract — always exits 0.
  poll  Emit the claude-ws state dir as one JSON array line (the
        claude-workspaces widget's poll side), sweeping dead sessions,
        orphan markers and stale subagent markers.
";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match argv.first().map(String::as_str) {
        Some("hook") => bsctl::hook::run(argv.get(1).map(String::as_str), &argv),
        Some("poll") => bsctl::poll::run(),
        Some("-h" | "--help") => {
            print!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("bsctl: unknown subcommand '{other}'");
            eprint!("{USAGE}");
            2
        }
        None => {
            eprint!("{USAGE}");
            2
        }
    };
    exit(code);
}
