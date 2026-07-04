# Claude Code

Claude Code is a self-managed install (its own updater); it is deliberately
absent from the package manifests. Config lives in the stowed `claude`
package (`~/.claude`): settings, hooks, statusline, skills, and the global
CLAUDE.md carrying the asks-queue posting norm.

## MCP registration (one-time)

Claude Code reads user-scope MCP servers only from `~/.claude.json` — a
state file this repo must never track — and ignores an `mcpServers` key in
`settings.json` (probed empirically on 2.1.200). So the battlestation MCP
server (the asks queue; contract in `ctl/src/lib.rs`) is registered once,
out of band:

```sh
claude mcp add --scope user battlestation -- "$HOME/.local/bin/bsctl" mcp --kind claude
```

Verify with `claude mcp list` (the server should report healthy — it needs
`make build` to have installed bsctl first).
