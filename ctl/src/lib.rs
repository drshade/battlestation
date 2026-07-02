//! bsctl — battlestation repo control tool.
//!
//! Implements the claude-ws status protocol, byte-for-byte compatible in
//! observable behavior with the two script references so consumers can switch
//! between script and binary freely:
//!
//! - hook side: `stow/home/bin/.local/bin/claude-ws-status.sh` (its header
//!   documents the file protocol; its body is the reference implementation)
//! - poll side: the `pollScript` python embedded in
//!   `stow/home/noctalia/.config/noctalia/plugins/claude-workspaces/BarWidget.qml`
//!
//! State lives as small JSON files in one flat dir,
//! `${XDG_RUNTIME_DIR:-/tmp}/claude-ws/`:
//! `<session_id>` session files and `<session_id>.<agent_id>` running-subagent
//! markers (marker mtime = start time, refreshed by the agent's tool calls).

pub mod hook;
pub mod poll;
pub mod proto;
pub mod sys;
