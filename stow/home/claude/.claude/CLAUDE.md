# This machine

This desktop runs a shared attention queue for every agent harness — the
`battlestation` MCP server (`bsctl mcp`; contract in the battlestation
repo's `ctl/src/lib.rs`). The rule it exists for:

**When you need the human's input, you MUST post an ask through the `ask`
tool rather than proceed on an assumption you couldn't defend.** Queue
depth or the human seeming busy is never a reason not to post — continuing
without needed feedback is the failure mode the queue prevents. Use
`notify` for anything the human should see even when you need no answer
(review requests, completion reports). If an ask times out, keep working
where you can and collect the answer later with `get_ask`; escalate by
raising your ask's urgency via `update_ask`, never by re-posting.
