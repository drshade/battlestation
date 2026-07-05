# This machine

This desktop runs a shared attention queue for every agent harness — the
`battlestation` MCP server (`bsctl mcp`; contract in the battlestation
repo's `ctl/src/lib.rs`). The human calls this queue **the Deck**: when
they tell you to report, check in, or "let someone know via the Deck",
they mean posting to it with the `ask`/`notify` tools. The rule it exists
for:

**When you need the human's input, you MUST post an ask through the `ask`
tool rather than proceed on an assumption you couldn't defend.** Queue
depth or the human seeming busy is never a reason not to post — continuing
without needed feedback is the failure mode the queue prevents. Use
`notify` for anything the human should see even when you need no answer
(review requests, completion reports). If an ask times out, keep working
where you can and collect the answer later with `get_ask`; escalate by
raising your ask's urgency via `update_ask`, never by re-posting.

Answers you didn't wait for arrive on their own: a turn-start hook injects
any answered ask of yours into your context (`[asks] ...` lines) the
moment your next turn begins. **Before re-raising one of your asks with
the human — mentioning it, asking for status, re-posting it — check it
first with `get_ask`: it may already be answered.**
