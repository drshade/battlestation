//! CLI shape only — clap stays a main.rs concern; the modules expose plain
//! functions. Protocol contracts live in lib.rs.

use std::process::exit;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "bsctl",
    about = "battlestation control tool",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Claude Code hook endpoint; hook-event JSON on stdin (always exits 0)
    Hook {
        /// waiting|thinking|tooling|clear|agent-start|agent-stop
        // A plain String, NOT a ValueEnum — hooks must never error loudly,
        // so unknown/missing verbs are validated internally (silent exit 0)
        // rather than by clap (loud exit 2). allow_hyphen_values keeps even
        // flag-shaped junk on that silent path.
        #[arg(allow_hyphen_values = true)]
        verb: Option<String>,
        /// Ignored — tolerated so a miswired hook command can never error
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        rest: Vec<String>,
    },
    /// Emit the claude-ws state dir as one JSON array line (widget poll side)
    Poll,
    /// Workspace display-order commands (navigate/move by display position)
    Ws {
        #[command(subcommand)]
        cmd: WsCmd,
    },
    /// Print Claude plan usage as one JSON line (cached, ttl 240s)
    Usage,
    /// Display state visibility & safe dpms management
    Display {
        #[command(subcommand)]
        cmd: DisplayCmd,
    },
    /// Generate shell completions on stdout
    Completions { shell: clap_complete::Shell },
}

#[derive(Subcommand)]
enum DisplayCmd {
    /// Per-output table + lid state + consistency warnings (always exits 0)
    Status {
        /// Emit raw structured JSON instead of the table
        #[arg(long)]
        json: bool,
    },
    /// DPMS an output on (safe: reads state, toggles only if off)
    On { output: String },
    /// DPMS an output off (safe: reads state, toggles only if on)
    Off { output: String },
    /// Recovery: reload config, reconcile lid, dpms-on enabled outputs
    Reset,
}

#[derive(Subcommand)]
enum WsCmd {
    /// Focus the workspace at a display position
    Goto {
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        pos: u32,
    },
    /// Send the active window to a display position
    Movewindow {
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        pos: u32,
        /// Move to the target workspace along with the window
        #[arg(long)]
        follow: bool,
    },
    /// Step focus (or the active window) through the display order
    Relative {
        #[arg(value_enum)]
        dir: Dir,
        /// Step the active window instead of just focus
        #[arg(long)]
        r#move: bool,
    },
    /// Write the preferred order (real workspace ids)
    #[command(allow_negative_numbers = true)]
    Set {
        #[arg(required = true)]
        ids: Vec<i64>,
    },
    /// Clear the preference (back to 1,2,3,... order)
    Reset,
    /// Print the raw preference file
    Get,
    /// Rename a workspace (empty/omitted name resets to its number)
    #[command(allow_negative_numbers = true)]
    Rename { id: i64, name: Option<String> },
    /// Print resolved "pos -> id (name)" (debug)
    Order,
}

#[derive(Clone, Copy, ValueEnum)]
enum Dir {
    Next,
    Prev,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Hook { verb, rest: _ } => {
            // Raw argv (not the parsed fields) feeds the debug log, matching
            // the sh reference's `argv: $*` line.
            let argv: Vec<String> = std::env::args().skip(1).collect();
            bsctl::hook::run(verb.as_deref(), &argv)
        }
        Cmd::Poll => bsctl::poll::run(),
        Cmd::Ws { cmd } => match cmd {
            WsCmd::Goto { pos } => bsctl::ws::goto(pos as usize),
            WsCmd::Movewindow { pos, follow } => bsctl::ws::movewindow(pos as usize, follow),
            WsCmd::Relative { dir, r#move } => {
                let delta = match dir {
                    Dir::Next => 1,
                    Dir::Prev => -1,
                };
                bsctl::ws::relative(delta, r#move)
            }
            WsCmd::Set { ids } => bsctl::ws::set(&ids),
            WsCmd::Reset => bsctl::ws::reset(),
            WsCmd::Get => bsctl::ws::get(),
            WsCmd::Rename { id, name } => bsctl::ws::rename(id, name.as_deref()),
            WsCmd::Order => bsctl::ws::order(),
        },
        Cmd::Usage => bsctl::usage::run(),
        Cmd::Display { cmd } => match cmd {
            DisplayCmd::Status { json } => bsctl::display::status(json),
            DisplayCmd::On { output } => bsctl::display::set_dpms(&output, true),
            DisplayCmd::Off { output } => bsctl::display::set_dpms(&output, false),
            DisplayCmd::Reset => bsctl::display::reset(),
        },
        Cmd::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "bsctl", &mut std::io::stdout());
            0
        }
    };
    exit(code);
}
