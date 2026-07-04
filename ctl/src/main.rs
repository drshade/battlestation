//! CLI shape only — clap stays a main.rs concern; the modules expose plain
//! functions. Protocol contracts live in lib.rs. The selector grammar
//! (--bs-id/--bs-rel/--ws-id/--display-id/--display-name) is enforced HERE,
//! per verb, via clap groups: a verb that can't take a display selector
//! simply doesn't declare one, so misuse is a usage error (exit 2), never a
//! runtime branch.

use std::process::exit;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

use bsctl::ws::{DisplaySel, RowFilter, Target, WsSel};

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
    /// Workspace verbs: focus/send by bs-id, ws-id or display; name, map, prefs
    Ws {
        #[command(subcommand)]
        cmd: WsCmd,
    },
    /// Display state, dpms and scale
    Display {
        #[command(subcommand)]
        cmd: DisplayCmd,
    },
    /// Agent-harness sessions: the hook endpoint (set) and the query (get)
    Agents {
        #[command(subcommand)]
        cmd: AgentsCmd,
    },
    /// The full state of the world: displays, battlespaces, prefs, agents, usage
    Status {
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Keep emitting the result whenever it changes (json: NDJSON; text: live view)
        #[arg(long)]
        stream: bool,
    },
    /// Generate shell completions on stdout
    Completions { shell: clap_complete::Shell },
}

/// `--stream` is orthogonal to `--format`: json streams NDJSON, text
/// streams frame for eyes (watch(1)-style clear-redraw on a tty, blank-line
/// separated when piped). The tty check happens once, here — never per
/// emission.
fn framing(format: Format) -> bsctl::stream::Framing {
    match format {
        Format::Json => bsctl::stream::Framing::Ndjson,
        Format::Text => bsctl::stream::Framing::Text {
            tty: unsafe { libc::isatty(1) } == 1,
        },
    }
}

// ---- selectors (clap-facing) --------------------------------------------------

/// The full workspace-or-display selector, exactly one required.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct AnySel {
    /// Battlespace id: 1-based position in the map (what SUPER+N means)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    bs_id: Option<u32>,
    /// Step through battlespace order from the active workspace (+n/-n)
    #[arg(long, allow_hyphen_values = true)]
    bs_rel: Option<i64>,
    /// Raw Hyprland workspace id (focusing a nonexistent id CREATES it)
    #[arg(long, allow_hyphen_values = true)]
    ws_id: Option<i64>,
    /// Display number: 1-based, leftmost first
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    display_id: Option<u32>,
    /// Output name (e.g. eDP-1)
    #[arg(long)]
    display_name: Option<String>,
}

impl AnySel {
    fn target(&self) -> Target {
        if let Some(n) = self.bs_id {
            Target::Ws(WsSel::Bs(n as usize))
        } else if let Some(d) = self.bs_rel {
            Target::Ws(WsSel::BsRel(d))
        } else if let Some(id) = self.ws_id {
            Target::Ws(WsSel::Ws(id))
        } else if let Some(n) = self.display_id {
            Target::Display(DisplaySel::Id(n as usize))
        } else {
            Target::Display(DisplaySel::Name(
                self.display_name.clone().unwrap_or_default(),
            ))
        }
    }
}

/// A workspace-only selector (bs-id or ws-id), exactly one required.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct WsOnlySel {
    /// Battlespace id: 1-based position in the map
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    bs_id: Option<u32>,
    /// Raw Hyprland workspace id
    #[arg(long, allow_hyphen_values = true)]
    ws_id: Option<i64>,
}

impl WsOnlySel {
    fn sel(&self) -> WsSel {
        match self.bs_id {
            Some(n) => WsSel::Bs(n as usize),
            None => WsSel::Ws(self.ws_id.unwrap_or_default()),
        }
    }
}

/// A display-only selector, exactly one required.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct DisplayOnlySel {
    /// Display number: 1-based, leftmost first
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    display_id: Option<u32>,
    /// Output name (e.g. eDP-1)
    #[arg(long)]
    display_name: Option<String>,
}

impl DisplayOnlySel {
    fn sel(&self) -> DisplaySel {
        match self.display_id {
            Some(n) => DisplaySel::Id(n as usize),
            None => DisplaySel::Name(self.display_name.clone().unwrap_or_default()),
        }
    }
}

/// The optional row filter (name get/rm, map get): at most one; none = all.
/// `--all` exists as the explicit spelling of the default.
#[derive(Args)]
#[group(required = false, multiple = false)]
struct FilterSel {
    /// Every workspace (the default)
    #[arg(long)]
    all: bool,
    /// Battlespace id: 1-based position in the map
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    bs_id: Option<u32>,
    /// Raw Hyprland workspace id
    #[arg(long, allow_hyphen_values = true)]
    ws_id: Option<i64>,
    /// Display number: 1-based, leftmost first
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    display_id: Option<u32>,
    /// Output name (e.g. eDP-1)
    #[arg(long)]
    display_name: Option<String>,
}

impl FilterSel {
    fn filter(&self) -> RowFilter {
        if let Some(n) = self.bs_id {
            RowFilter::Ws(WsSel::Bs(n as usize))
        } else if let Some(id) = self.ws_id {
            RowFilter::Ws(WsSel::Ws(id))
        } else if let Some(n) = self.display_id {
            RowFilter::Display(DisplaySel::Id(n as usize))
        } else if let Some(name) = &self.display_name {
            RowFilter::Display(DisplaySel::Name(name.clone()))
        } else {
            RowFilter::All // --all or nothing at all
        }
    }
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum Format {
    Text,
    Json,
}

// ---- ws ------------------------------------------------------------------------

#[derive(Subcommand)]
enum WsCmd {
    /// Focus a workspace or display (never mutates)
    Focus {
        #[command(flatten)]
        sel: AnySel,
    },
    /// Move the active window or workspace somewhere (--focus to follow)
    Send {
        #[command(subcommand)]
        cmd: SendCmd,
    },
    /// Get/set/remove workspace display names
    Name {
        #[command(subcommand)]
        cmd: NameCmd,
    },
    /// The battlespace map: bs-id -> ws-id
    Map {
        #[command(subcommand)]
        cmd: MapCmd,
    },
    /// Sparse workspace->display preferences (applied when the display arrives)
    Prefs {
        #[command(subcommand)]
        cmd: PrefsCmd,
    },
}

#[derive(Subcommand)]
enum SendCmd {
    /// Send the active window to a workspace (or a display's active workspace)
    Window {
        #[command(flatten)]
        sel: AnySel,
        /// Follow the window (keyboard moves with it)
        #[arg(long)]
        focus: bool,
    },
    /// Send the active workspace to a display (keeps its ws-id; stamps its preference)
    Workspace {
        #[command(flatten)]
        sel: DisplayOnlySel,
        /// Follow the workspace to its new display
        #[arg(long)]
        focus: bool,
    },
}

#[derive(Subcommand)]
enum NameCmd {
    /// List names: `bs <n>  ws <id>  "<name>"` (default: every workspace)
    Get {
        #[command(flatten)]
        filter: FilterSel,
    },
    /// Name one workspace
    Set {
        #[command(flatten)]
        sel: WsOnlySel,
        /// The new display name
        #[arg(long)]
        name: String,
    },
    /// Reset names back to the workspace number (default: every workspace)
    Rm {
        #[command(flatten)]
        filter: FilterSel,
    },
}

#[derive(Subcommand)]
enum MapCmd {
    /// The resolved join: bs-id, ws-id, name, display, windows, active
    Get {
        #[command(flatten)]
        filter: FilterSel,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Keep emitting the result whenever it changes (json: NDJSON; text: live view)
        #[arg(long)]
        stream: bool,
    },
    /// Write the map: ws-ids in battlespace order (the FULL list)
    #[command(allow_negative_numbers = true)]
    Set {
        #[arg(required = true)]
        ids: Vec<i64>,
    },
    /// Clear the map (back to ascending ws-id order)
    Reset,
}

#[derive(Subcommand)]
enum PrefsCmd {
    /// List preferences with presence/liveness annotations
    Get {
        #[command(flatten)]
        filter: PrefsFilterSel,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Keep emitting the result whenever it changes (json: NDJSON; text: live view)
        #[arg(long)]
        stream: bool,
    },
    /// Home a workspace on a display (--display-name may be absent: pre-declare)
    Add {
        #[command(flatten)]
        sel: WsOnlySel,
        #[command(flatten)]
        display: DisplayOnlySel,
    },
    /// Drop one preference (or --all of them)
    Rm {
        #[command(flatten)]
        sel: PrefsRmSel,
    },
    /// Move workspaces to their preferred displays now
    Reconcile,
}

/// prefs get's optional narrowing (bs-id/ws-id only — prefs are per-workspace).
#[derive(Args)]
#[group(required = false, multiple = false)]
struct PrefsFilterSel {
    /// Every preference (the default)
    #[arg(long)]
    all: bool,
    /// Battlespace id: 1-based position in the map
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    bs_id: Option<u32>,
    /// Raw Hyprland workspace id
    #[arg(long, allow_hyphen_values = true)]
    ws_id: Option<i64>,
}

impl PrefsFilterSel {
    fn sel(&self) -> Option<WsSel> {
        if let Some(n) = self.bs_id {
            Some(WsSel::Bs(n as usize))
        } else {
            self.ws_id.map(WsSel::Ws)
        }
    }
}

/// prefs rm's required choice: one workspace or --all.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct PrefsRmSel {
    /// Drop every preference
    #[arg(long)]
    all: bool,
    /// Battlespace id: 1-based position in the map
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    bs_id: Option<u32>,
    /// Raw Hyprland workspace id
    #[arg(long, allow_hyphen_values = true)]
    ws_id: Option<i64>,
}

impl PrefsRmSel {
    fn sel(&self) -> Option<WsSel> {
        if let Some(n) = self.bs_id {
            Some(WsSel::Bs(n as usize))
        } else {
            self.ws_id.map(WsSel::Ws) // None here MEANS --all (clap enforced)
        }
    }
}

// ---- display ---------------------------------------------------------------------

#[derive(Subcommand)]
enum DisplayCmd {
    /// Per-output table (+ display ids) + lid state + consistency warnings
    Get {
        #[command(flatten)]
        filter: DisplayFilterSel,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Keep emitting the result whenever it changes (json: NDJSON; text: live view)
        #[arg(long)]
        stream: bool,
    },
    /// Mutations: dpms and scale
    Set {
        #[command(subcommand)]
        cmd: DisplaySetCmd,
    },
    /// Recovery: reload config, reconcile lid, dpms-on enabled outputs
    Reset,
}

/// display get's optional narrowing.
#[derive(Args)]
#[group(required = false, multiple = false)]
struct DisplayFilterSel {
    /// Every output (the default)
    #[arg(long)]
    all: bool,
    /// Display number: 1-based, leftmost first
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    display_id: Option<u32>,
    /// Output name (e.g. eDP-1)
    #[arg(long)]
    display_name: Option<String>,
}

impl DisplayFilterSel {
    fn sel(&self) -> Option<DisplaySel> {
        if let Some(n) = self.display_id {
            Some(DisplaySel::Id(n as usize))
        } else {
            self.display_name.clone().map(DisplaySel::Name)
        }
    }
}

#[derive(Subcommand)]
enum DisplaySetCmd {
    /// DPMS an output (safe: reads state, toggles only if it differs)
    Dpms {
        #[command(flatten)]
        sel: DisplayOnlySel,
        #[command(flatten)]
        state: DpmsState,
    },
    /// Scale a display: ladder steps, an explicit value, or back to auto
    Scale {
        #[command(flatten)]
        sel: ScaleSel,
        #[command(flatten)]
        action: ScaleActionArgs,
    },
}

/// dpms's required state, spelled as flags per the design sketch.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct DpmsState {
    /// Power the output on
    #[arg(long)]
    on: bool,
    /// Power the output off
    #[arg(long)]
    off: bool,
}

/// scale's OPTIONAL selector — none targets the focused monitor (what the
/// zoom keybinds mean).
#[derive(Args)]
#[group(required = false, multiple = false)]
struct ScaleSel {
    /// Display number: 1-based, leftmost first
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    display_id: Option<u32>,
    /// Output name (e.g. eDP-1)
    #[arg(long)]
    display_name: Option<String>,
}

impl ScaleSel {
    fn sel(&self) -> Option<DisplaySel> {
        if let Some(n) = self.display_id {
            Some(DisplaySel::Id(n as usize))
        } else {
            self.display_name.clone().map(DisplaySel::Name)
        }
    }
}

/// scale's required action, exactly one.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct ScaleActionArgs {
    /// One ladder rung up
    #[arg(long)]
    up: bool,
    /// One ladder rung down
    #[arg(long)]
    down: bool,
    /// Back to scale = auto (drops the saved rung)
    #[arg(long)]
    reset: bool,
    /// An explicit scale (Hyprland snaps it to its 1/120 grid)
    #[arg(long)]
    value: Option<f64>,
}

impl ScaleActionArgs {
    fn action(&self) -> bsctl::scale::Action {
        if self.up {
            bsctl::scale::Action::Up
        } else if self.down {
            bsctl::scale::Action::Down
        } else if self.reset {
            bsctl::scale::Action::Reset
        } else {
            bsctl::scale::Action::Value(self.value.unwrap_or_default())
        }
    }
}

// ---- agents ----------------------------------------------------------------------

#[derive(Subcommand)]
enum AgentsCmd {
    /// The hook endpoint; hook-event JSON on stdin (always exits 0)
    Set {
        /// --kind <harness> waiting|thinking|tooling|clear|subagent-start|subagent-stop [--session-id <s>]
        // Raw tokens, NOT clap-typed args — hooks must never error loudly,
        // so --kind, the verb and --session-id are validated internally
        // (silent exit 0) rather than by clap: a clap option with a missing
        // value exits 2 loudly, and unknown/missing verbs would too under a
        // ValueEnum. trailing_var_arg + allow_hyphen_values keeps every
        // token (flag-shaped junk included) on the silent internal path.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Query the live sessions (sweeps dead ones on the way)
    Get {
        /// Only this harness kind
        #[arg(long)]
        kind: Option<String>,
        /// Only this session id
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Keep emitting the result whenever it changes (json: NDJSON; text: live view)
        #[arg(long)]
        stream: bool,
    },
    /// Plan usage per harness kind (cached; one provider today: claude)
    Usage {
        /// Only this harness kind
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
}

fn main() {
    // Die quietly mid-pipeline like every Unix filter (`status --format
    // json | head` must not panic): Rust ignores SIGPIPE by default, which
    // turns a closed pipe into a println! panic. The stream engine
    // additionally maps EPIPE to a clean exit for environments that BLOCK
    // the signal — a disposition can be reset here, an inherited mask
    // can't.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Ws { cmd } => match cmd {
            WsCmd::Focus { sel } => bsctl::ws::focus(&sel.target()),
            WsCmd::Send { cmd } => match cmd {
                SendCmd::Window { sel, focus } => bsctl::ws::send_window(&sel.target(), focus),
                SendCmd::Workspace { sel, focus } => bsctl::ws::send_workspace(&sel.sel(), focus),
            },
            WsCmd::Name { cmd } => match cmd {
                NameCmd::Get { filter } => bsctl::ws::name_get(&filter.filter()),
                NameCmd::Set { sel, name } => bsctl::ws::name_set(&sel.sel(), &name),
                NameCmd::Rm { filter } => bsctl::ws::name_rm(&filter.filter()),
            },
            WsCmd::Map { cmd } => match cmd {
                MapCmd::Get {
                    filter,
                    format,
                    stream,
                } => {
                    let f = filter.filter();
                    if stream {
                        let json = format == Format::Json;
                        bsctl::stream::run(
                            move || {
                                if json {
                                    bsctl::ws::map_json(&f)
                                } else {
                                    bsctl::ws::map_text(&f)
                                }
                            },
                            framing(format),
                        )
                    } else {
                        bsctl::ws::map_get(&f, format == Format::Json)
                    }
                }
                MapCmd::Set { ids } => bsctl::ws::map_set(&ids),
                MapCmd::Reset => bsctl::ws::map_reset(),
            },
            WsCmd::Prefs { cmd } => match cmd {
                PrefsCmd::Get {
                    filter,
                    format,
                    stream,
                } => {
                    let sel = filter.sel();
                    if stream {
                        let json = format == Format::Json;
                        bsctl::stream::run(
                            move || {
                                if json {
                                    bsctl::ws::prefs_json(sel.as_ref())
                                } else {
                                    bsctl::ws::prefs_text(sel.as_ref())
                                }
                            },
                            framing(format),
                        )
                    } else {
                        bsctl::ws::prefs_get(sel.as_ref(), format == Format::Json)
                    }
                }
                PrefsCmd::Add { sel, display } => bsctl::ws::prefs_add(&sel.sel(), &display.sel()),
                PrefsCmd::Rm { sel } => bsctl::ws::prefs_rm(sel.sel().as_ref()),
                PrefsCmd::Reconcile => bsctl::ws::reconcile(),
            },
        },
        Cmd::Display { cmd } => match cmd {
            DisplayCmd::Get {
                filter,
                format,
                stream,
            } => {
                let sel = filter.sel();
                if stream {
                    let json = format == Format::Json;
                    bsctl::stream::run(
                        move || {
                            if json {
                                bsctl::display::get_json(sel.as_ref())
                            } else {
                                bsctl::display::get_text(sel.as_ref())
                            }
                        },
                        framing(format),
                    )
                } else {
                    bsctl::display::get(sel.as_ref(), format == Format::Json)
                }
            }
            DisplayCmd::Set { cmd } => match cmd {
                DisplaySetCmd::Dpms { sel, state } => {
                    bsctl::display::set_dpms(&sel.sel(), state.on && !state.off)
                }
                DisplaySetCmd::Scale { sel, action } => {
                    bsctl::scale::run(action.action(), sel.sel().as_ref())
                }
            },
            DisplayCmd::Reset => bsctl::display::reset(),
        },
        Cmd::Agents { cmd } => match cmd {
            AgentsCmd::Set { args } => {
                // Raw argv (not the parsed fields) feeds the debug log,
                // matching the sh reference's `argv: $*` line.
                let argv: Vec<String> = std::env::args().skip(1).collect();
                bsctl::agents::set(&args, &argv)
            }
            AgentsCmd::Get {
                kind,
                session_id,
                format,
                stream,
            } => {
                if stream {
                    let json = format == Format::Json;
                    bsctl::stream::run(
                        move || {
                            let (k, s) = (kind.as_deref(), session_id.as_deref());
                            Ok(if json {
                                bsctl::agents::get_json(k, s)
                            } else {
                                bsctl::agents::get_text(k, s)
                            })
                        },
                        framing(format),
                    )
                } else {
                    bsctl::agents::get(
                        kind.as_deref(),
                        session_id.as_deref(),
                        format == Format::Json,
                    )
                }
            }
            AgentsCmd::Usage { kind, format } => {
                bsctl::usage::get(kind.as_deref(), format == Format::Json)
            }
        },
        Cmd::Status { format, stream } => {
            if stream {
                let json = format == Format::Json;
                bsctl::stream::run(
                    move || {
                        let w = bsctl::world::snapshot();
                        Ok(if json {
                            w.to_string()
                        } else {
                            bsctl::world::render_text(&w)
                        })
                    },
                    framing(format),
                )
            } else if format == Format::Json {
                println!("{}", bsctl::world::snapshot());
                0
            } else {
                print!("{}", bsctl::world::render_text(&bsctl::world::snapshot()));
                0
            }
        }
        Cmd::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "bsctl", &mut std::io::stdout());
            0
        }
    };
    exit(code);
}
