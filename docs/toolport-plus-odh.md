# Toolport+ and an ODH-shaped MCP server

What Toolport+ adds on top of upstream Toolport so a long-running stdio MCP server that asks questions,
reports progress and returns links (the Odoo dev harness, server id `odh`, launched with
`uv run --directory <repo>/mcp-server odh-mcp`) works through the gateway. Everything here is off or
inert until a server asks for it.

## Switches on a server

Stored in the registry entry next to upstream's own keys. Set them with `toolportctl server edit <id>` or the
`servers_*` self-MCP tools; registries from the 1.x fork carry them unchanged.

| Key                         | Default | Effect                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --------------------------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `declareClientCapabilities` | off     | The server's `initialize` declares the `roots`, `sampling` and `elicitation` its clients declared (the union of every client seen; `experimental`, `extensions` and `tasks` are not relayed). A server that connected before any client did is restarted once when a client declares more, not while a call is running on it; the next client connect tries again. Needed by servers that only ask the user when their client says it can be asked |
| `forwardInstructions`       | off     | The server's `instructions` follow the gateway's own text under `## Instructions from server "<name>"`. Up to 4096 characters per server, control characters stripped, forged gateway markers neutralised, dropped when they trip the injection scan; only servers in the asking client's scope. Servers appear in registry order                                                                                                                  |
| `maxRequestTimeoutMs`       | 1 hour  | Absolute cap on one call to a stdio server that keeps reporting progress (1 ms to 24 h)                                                                                                                                                                                                                                                                                                                                                            |

## Without a switch

- **Progress keeps a call alive.** For a stdio server, a `notifications/progress` for the call's own
  `progressToken` moves the deadline to the progress time plus `requestTimeoutMs`, never past
  `maxRequestTimeoutMs`. Time spent waiting for the client to answer a server request (a confirmation) is
  charged to neither. A call without a token, or without progress, times out exactly as upstream does.
  HTTP and SSE servers keep a fixed deadline.
- **The stdio adapter follows.** The adapter used to cut every request at 600 s. A `tools/call`,
  `resources/read` or `prompts/get` now gets the longest stdio cap (or request timeout) of the enabled servers
  plus a minute, never less than 600 s.
- **`uv run` and `uv tool run` are download launchers.** The first `initialize` waits up to 120 s, like `npx` and
  `uvx`, and the desktop app pre-warms them. Other `uv` subcommands keep the 10 s budget.
- **A `resource_link` can be read.** The gateway remembers the links of successful tool results (256, oldest
  first out, URIs over 2048 bytes skipped) and routes a read of one to the server that returned it. A listed or
  templated URI keeps its owner, the first server to link a URI keeps it, and a client scoped away from the
  server cannot read its links. Links are remembered per gateway process, not per session.

## What follows upstream

The 1.x fork's 20 s full-discovery cold-start wait and its "keep `structuredContent` for tools with an
`outputSchema`" path are not carried over (D-122): upstream's 2 s bound with supervised lazy starts and its
handling of output schemas apply.

## Where the code is

Decision logic in `plus/core` (`launcher.rs`, `progress.rs`, `handshake.rs`, `resource_links.rs`); hooks in
`src-tauri/src/seam.rs` and `seam/handshake.rs`; one-line calls from `downstream.rs`, `router.rs`,
`stdio_adapter.rs` and the gateway binary, each listed with its reason in `scripts/fork/patch-budget.txt`.
Tests run against a fake stdio server, `plus/fixtures/stdio/fake-mcp.mjs`.
