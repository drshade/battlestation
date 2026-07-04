# Codex CLI

Codex (OpenAI's terminal agent) is a self-managed standalone install, not
package-managed — deliberately no `packages/*.txt` entry: the binary lives
outside pacman/npm as versioned releases under `~/.codex/packages/standalone/`
with `~/.local/bin/codex` symlinked to the current one, and `codex update`
replaces it in place. Install per the upstream instructions
(<https://developers.openai.com/codex/cli>), then `codex login`.

Config is the stowed `~/.codex/config.toml` (`stow/home/codex`), which
carries the agent-session status hooks (`bsctl agents set --kind codex …` — the
protocol contract is in `ctl/src/lib.rs`). Two out-of-band steps:

- **Trust the hooks (one-time).** Codex refuses non-managed command hooks
  until they are reviewed: open `codex` and approve them via `/hooks`, else
  the status widget never sees Codex sessions.
- Codex rewrites `config.toml` at runtime (project trust tables, `[tui.*]`
  state). That churn is projected out by the `codex-config` clean filter
  (`bin/filters/codex-config`, merged-config pattern per AGENTS.md); a fresh
  clone needs `make fix` once to activate it.
