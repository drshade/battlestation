# Antigravity CLI (`agy`)

Antigravity (Google's terminal agent) is a self-managed standalone install,
not package-managed — deliberately no `packages/*.txt` entry: a single Go
binary at `~/.local/bin/agy` (no pacman owner) that `agy update` replaces in
place. Install per the upstream Antigravity CLI instructions, then run `agy`
once and complete the Google sign-in (credentials land in
`~/.gemini/oauth_creds.json`).

`~/.gemini/` is otherwise runtime state and secrets — never stow it wholesale.
The `antigravity` package owns two files under `~/.gemini/config/`:
`hooks.json`, carrying the agent-session status hooks (`bsctl agents set
--kind agy …`; the protocol contract is in `ctl/src/lib.rs` — agy's hook
payloads are camelCase protojson keyed by `conversationId`, which bsctl
accepts as the session key), and `mcp_config.json`, registering the
battlestation MCP server (the asks queue; via `sh -c` because agy execs MCP
commands directly and would not expand `$HOME`). agy rewrites neither at
runtime, so no clean filter is needed.

- **No hook trust gate.** Unlike Codex's `/hooks` approval, agy executes
  `hooks.json` commands without any review step — the hooks fire on first
  use. Anything that can write that file runs code inside every agy session,
  so it stays repo-owned (a symlink into this repo, restored by `make fix`).
- **No subagent hooks.** agy supports subagents but exposes no
  SubagentStart/SubagentStop events (they are ignored in `hooks.json`), so
  the status widget shows an agy commander bot but never its squad.
- **No global instructions file.** agy reads GEMINI.md/AGENTS.md
  hierarchically per project only — there is no user-level equivalent of
  Claude's `~/.claude/CLAUDE.md` or Codex's `~/.codex/AGENTS.md` — so the
  asks-queue posting norm reaches agy sessions through the MCP tool
  descriptions alone.
- **No turn-boundary answer injection.** agy's hook surface has no
  UserPromptSubmit event and no context-injection output (no
  `additionalContext` anywhere in the binary, probed 1.0.16), so the
  `bsctl asks inbox` delivery hook the other harnesses run cannot be
  wired: agy agents collect late answers via `get_ask` only (the norm the
  tool descriptions carry).
- **Between-turns status is approximate.** agy fires a trailing PostToolUse
  (→ thinking) after Stop — which is why Stop maps to `waiting` rather than
  `clear` (a clear would be resurrected as "thinking" moments later). An
  idle session can still briefly read "thinking" until the next event; the
  session file is swept when the `agy` process dies.
