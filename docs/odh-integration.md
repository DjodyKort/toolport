# ODH integration

What the Toolport+ gateway does today for an ODH-shaped downstream MCP server, answering the
"odh-best" hand-over (issue #29). This page states facts only. Every claim cites `file:line`
(against this branch) or a test name. Feature work that follows from the gap list is decided
separately (Q-020, issue #30).

Everything here uses synthetic data: server id `odh`, command `uv`, args
`run --directory <repo>/mcp-server odh-mcp`, `FAKE-*` values, no credentials.

Two test files back the tables. Run them with:

```sh
cd src-tauri
cargo test --no-default-features --test odh_integration
cargo test --no-default-features --lib odh_tests
```

`odh_integration` drives the real `toolport-gateway --stdio-adapter` (adapter plus shared host
daemon, the default topology) against `mock-mcp-server` with `MOCK_MCP_PROFILE=odh`.
`odh_tests` (`src-tauri/src/plus/import_mcpm/odh_tests.rs`) covers the registry, cutover, ctl and
screening rows and checks that the two JSON examples below are exactly what the code writes.
The deadline rows (B3) are also backed by unit tests in `downstream.rs`, `stdio_adapter.rs` and
`registry.rs`, listed at the end of the Test index.

## A1. The registry file

**Name and location.** `registry.json` inside the data directory.

| Source                                          | Effect                                                                                                                                                                                        | Where                                                                      |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `TOOLPORT_DATA_DIR` (legacy `CONDUIT_DATA_DIR`) | the full data-dir path                                                                                                                                                                        | `registry.rs:2920`                                                         |
| otherwise `dirs::config_dir()/Toolport`         | `~/.config/Toolport` on Linux, `~/Library/Application Support/Toolport` on macOS; debug builds use `Toolport-dev`; an existing legacy `Conduit` leaf is reused when `Toolport` does not exist | `registry.rs:2919-2946`, `brand.rs:64,91`                                  |
| `TOOLPORT_REGISTRY` (legacy `CONDUIT_REGISTRY`) | overrides the file path only; the gateway and `toolportctl` both honor it                                                                                                                     | `registry.rs:4067`, `plus/ctl/commands.rs:20`, `plus/registry_ro.rs:20,27` |

`registry_path()` is `<data dir>/registry.json` (`registry.rs:3015`). The gateway and `toolportctl`
both go through `resolved_path()` (`registry.rs:4067`), which returns `TOOLPORT_REGISTRY` when it is
set and `registry_path()` otherwise; `status` still reports the data dir as `dataDir`. Pinned by lib
`ctl_reads_the_file_toolport_registry_names_like_the_gateway` and
`reads_follow_toolport_registry_like_the_loader`, and by e2e
`toolportctl_and_the_gateway_read_the_same_file_under_toolport_registry`. A sibling `registry.json.bak` holds the last-known-good copy and is used for recovery
(`registry.rs:3078`).

**Top-level shape.** One JSON object, camelCase keys, written by `registry::save_to`.

- `version`, `servers[]`, `profiles[]`, `activeProfileId`: the core.
- `clientScopes` (client id to profile id), `clientDiscovery` (client id to `full`/`lazy`/`grouped`),
  `clientManagedEntries` (what Toolport wrote into each client config).
- Settings such as `lazyDiscovery`, `contentDefense`, `piiRedaction`, `resultBudgets` (per server id,
  bytes, `0` disables shaping; `registry.rs:1199`), `gatewayInstructions`.
- `plus`: Toolport+ state (for example `plus.selfmcp.enabledIn`). Upstream does not know this key; it
  round-trips through the flattened `unknown_fields` on `Registry`, `ServerEntry` and `Profile`
  (`registry.rs:1316`, `674`, `771`).

A server entry: `id`, `name`, `transport`, `command`, `args`, `env[]` (`key`, `secret`, optional
`value`), `cwd`, `source`, optional `requestTimeoutMs`, `maxRequestTimeoutMs` and
`initializeTimeoutMs` (`registry.rs:654,663,669`; B3 explains the first two), the two opt-in switches
`declareClientCapabilities` and `forwardInstructions` (B6; written only when on), plus the plus-only
extras in `unknown_fields`.

**Env values.** A key that looks secret is stored as `{"key": ..., "secret": true}` with the value in
the vault under `<server id>::<KEY>`; any other key keeps its value in the file in clear text. In the
example `ODOO_URL` is not secret-shaped, so its value sits in `registry.json`, and `ODOO_API_KEY` is
vaulted. Pinned by `the_registry_file_keeps_non_secret_env_values_and_vaults_the_secret_ones`.

**Sanitized example.** Generated by running the real cutover (`import_mcpm::run`) on a synthetic
mcpm tree with one server `odh-mcp` and one `claude-code` client entry. Only three things were
rewritten: timestamps become `"<timestamp>"` (the real field is an integer), install paths become
`<install-dir>`, and `env[].value` is removed. `toolport-plus-self` is the Toolport+ self server that
the run registers. `the_documented_registry_and_client_entry_are_what_the_cutover_writes` fails when
this block drifts from the code.

<!-- example:registry -->

```json
{
  "clientDiscovery": {
    "claude-code": "full"
  },
  "clientManagedEntries": {
    "claude-code": {
      "args": [],
      "command": "<install-dir>/toolport-gateway",
      "env": {
        "TOOLPORT_CLIENT_ID": "claude-code",
        "TOOLPORT_PROFILE": "claude-code"
      },
      "transport": "stdio",
      "updatedAt": "<timestamp>"
    }
  },
  "clientScopes": {
    "claude-code": "claude-code"
  },
  "plus": {
    "selfmcp": {
      "enabledIn": ["default", "claude-code"]
    }
  },
  "profiles": [
    {
      "enabledServerIds": ["toolport-plus-self"],
      "id": "default",
      "name": "Default"
    },
    {
      "enabledServerIds": ["odh", "toolport-plus-self"],
      "id": "claude-code",
      "name": "claude-code"
    }
  ],
  "servers": [
    {
      "args": ["run", "--directory", "<repo>/mcp-server", "odh-mcp"],
      "command": "uv",
      "env": [
        {
          "key": "ODOO_API_KEY",
          "secret": true
        },
        {
          "key": "ODOO_URL",
          "secret": false
        }
      ],
      "id": "odh",
      "name": "odh-mcp",
      "source": "imported:mcpm",
      "transport": "stdio"
    },
    {
      "args": [],
      "command": "<install-dir>/toolport-selfmcp",
      "env": [],
      "id": "toolport-plus-self",
      "name": "toolport-plus-self",
      "source": "plus:selfmcp",
      "transport": "stdio"
    }
  ]
}
```

## A2. Learning what Toolport starts for `odh`, without the keychain

Three supported routes, in order of preference. All three were run with `TOOLPORT_SECRET_KEY` and
`DBUS_SESSION_BUS_ADDRESS` unset: exit code 0, nothing created in the data dir, no vault touched.

1. **Read `registry.json` directly.** It holds the exact `command`, `args`, `cwd`, env key names and
   non-secret env values. No secret value is ever in it (A1).
2. **`toolportctl server info odh [--json]`** (`plus/ctl/server.rs:290`). Prints `id`, `name`,
   `transport`, `command`, `args`, `url`, `cwd`, `source`, `env` as `{key, secret}` only,
   `disabledTools`, `declareClientCapabilities`, `forwardInstructions` and the `profiles` that enable it. Reads through `registry_ro::read_at`
   (`plus/ctl/commands.rs:18-33`), which never writes and never opens the vault. Env values are never
   printed, secret or not. `command`, `args` and `cwd` are printed verbatim, so a secret placed in
   `args` would be shown; the cutover keeps secrets in `env`. Pinned by
   `ctl_server_info_names_the_launch_line_and_env_keys_but_never_env_values`. Human form:
   `Target: uv run --directory <repo>/mcp-server odh-mcp`. `server ls` shows `enabled` relative to
   the active profile only, so `odh` reads `false` while the active profile is `default`; use
   `server info` and its `profiles` list.
3. **`toolportctl status` / `doctor`** (`commands.rs:97,154`). Counts, data dir, registry path and
   readability, active profile, gateway binary. `secretsBackend` is a label derived from whether
   `TOOLPORT_SECRET_KEY` is set (`commands.rs:37`), not a vault read.

Not for this question: `toolportctl mcp doctor` checks only the Toolport+ self server
(`plus/ctl/mcp.rs:106`). The `toolport_status` meta-tool (`bin/toolport-gateway.rs:1272`) reports the
live gateway but needs a running gateway, and a gateway that spawns `odh` reads the vault for secret
env keys. Resolved secret values exist only inside the spawned child.

If `TOOLPORT_REGISTRY` points the gateway at another file, `toolportctl` reads that file too (B2).

## A3. The cutover surface

After `scripts/cutover/cutover.sh` (calls `toolportctl import mcpm`, then `import rename-refs`,
`doctor`, `mcp doctor`) each client holds ONE `toolport` entry and the registry holds the servers and a
per-client profile. Written by `import_mcpm::run` (`plus/import_mcpm/run.rs:491`) through
`clients::apply_import`; the entry shape comes from `clients::gateway_entry`
(`clients.rs:5337`). Example for Claude Code (`~/.claude.json`), install path sanitized:

<!-- example:client-entry -->

```json
{
  "mcpServers": {
    "toolport": {
      "args": [],
      "command": "<install-dir>/toolport-gateway",
      "env": {
        "TOOLPORT_CLIENT_ID": "claude-code",
        "TOOLPORT_PROFILE": "claude-code"
      }
    }
  }
}
```

Following it to the registry entry:

1. `env.TOOLPORT_CLIENT_ID` is `claude-code`. The gateway resolves the live profile from
   `clientScopes["claude-code"]`, which is `"claude-code"`; `TOOLPORT_PROFILE` is only the initial
   value (`clients.rs:5360-5365`).
2. `profiles[id = "claude-code"].enabledServerIds` lists `"odh"` (and `toolport-plus-self`).
3. `servers[id = "odh"]` is the entry from A1: `uv run --directory <repo>/mcp-server odh-mcp`.
4. `clientDiscovery["claude-code"]` is `"full"` (`plus/import_mcpm/clients.rs:190`): the cutover sets
   Claude Code to full discovery and every other client to `lazy`, which changes the tool names a
   client sees (B2, tool names).
5. `clientManagedEntries["claude-code"]` records what was written, so a re-run updates it and
   `scripts/cutover/rollback.sh` can undo it.

The server id is `odh` because `generic()` strips a trailing `-mcp` from the mcpm name
(`plus/import_mcpm/ids.rs:36`) or because a short-id table maps it. Pinned by
`import_path_maps_and_accepts_the_uv_run_directory_form`.

## B2. Support table

Status: **supported**, **partial**, **not supported**. Rows run against the `odh` mock profile; "e2e"
tests are in `tests/odh_integration.rs`.

| Row                                             | Status    | What happens                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ----------------------------------------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Protocol version negotiation                    | supported | Upstream: a request for `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25` is answered with that revision; `2026-07-28` is also in the supported list (modern requests are stateless and handled on a separate path, not exercised here); anything else gets `2025-06-18`. Downstream: the gateway offers `2025-06-18` and accepts whatever revision the server answers; a server that rejects `initialize` is probed with `server/discover` for the modern era.                                                                                                                                                                                                                                                                                                                                                                                                                           | `toolport-gateway.rs:494,7929-7945`; `downstream.rs:289,5605-5629`; e2e `the_gateway_answers_every_known_revision_with_that_revision`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Declared client capabilities                    | partial   | The gateway advertises `tools`, `resources`, `prompts`, `completions` (no `logging`, `tasks`, `elicitation`, `sampling`). Toward the downstream server it declares `capabilities: {}` in legacy `initialize` even when its client declared `elicitation`, `roots` and `sampling`, unless the server is set to `declareClientCapabilities` (next row, B6).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | `toolport-gateway.rs:599-620`; `downstream.rs:5608`; e2e `the_gateway_advertises_what_it_serves_and_declares_nothing_downstream`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Client capabilities downstream (G1)             | supported | Per-server `declareClientCapabilities`, default off. Off: the legacy `initialize` declares `{}` as before. On: it declares the `roots`, `sampling` and `elicitation` the client declared, and nothing else (`experimental`, `extensions` and `tasks` are not relayed, so not declared). A server connects before any client does, so one set to it is re-spawned once a client's declaration is known (the client's `initialize` waits up to 10 s for that); the declaration is the union of the clients seen and only grows. A modern-era (2026-07-28) downstream gets request-scoped capabilities anyway.                                                                                                                                                                                                                                                                                    | `handshake.rs`; `toolport-gateway.rs` `declare_client_capabilities_downstream`; e2e `with_declare_client_capabilities_the_downstream_initialize_carries_exactly_the_clients`, `with_declare_client_capabilities_a_server_that_gates_elicitation_asks_and_the_answer_comes_back`, `with_the_opt_in_off_every_downstream_initialize_stays_empty_and_a_gating_server_never_asks`, `a_modern_client_that_declares_elicitation_per_request_reaches_a_gating_legacy_server`                                                                                                                                                                                                                                                                                                                                                                            |
| `outputSchema` / `structuredContent`            | supported | Full discovery: `outputSchema` reaches the client unchanged apart from brand-spoof string neutralization, and `structuredContent` passes through. In lazy discovery (the registry default) `structuredContent` flows through `toolport_call_tool` the same way.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `toolport-gateway.rs` `neutralize_listed_tool`; e2e `tool_names_carry_the_server_prefix_and_keep_the_output_schema`, `in_lazy_mode_the_odh_tools_are_reached_through_toolport_call_tool`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `outputSchema` in lazy search (G8)              | supported | The tool list of a lazy client shows only meta-tools, and `toolport_search_tools` returns name, a truncated description (500 chars for the top hit, 140 for the rest) and the top hit's `inputSchema`. The top hit also carries its `outputSchema` when the tool declares one, so a client can check the `structuredContent` of the call against it. Only the top hit gets it (an exact-name search puts that tool first); the menu entries after it carry neither schema. An `outputSchema` over 8192 serialized bytes is left out and the hit says `"outputSchemaOmitted": true`, with a note in the lead text. The schema is neutralized like the `inputSchema`. The description caps do not change.                                                                                                                                                                                        | `toolport-gateway.rs` `project_budgeted`, `TOP_OUTPUT_SCHEMA_MAX_BYTES`; e2e `lazy_search_gives_the_top_hit_its_output_schema_and_the_menu_entries_none`, `in_lazy_mode_the_odh_tools_are_reached_through_toolport_call_tool`; `toolport-gateway` bin `the_top_hit_carries_its_output_schema_and_menu_entries_do_not`, `a_top_hit_without_a_usable_output_schema_gets_neither_the_field_nor_the_flag`, `an_output_schema_is_kept_up_to_its_byte_cap_and_left_out_flagged_past_it`, `a_top_hits_output_schema_cannot_speak_as_the_gateway`, `search_text_says_when_a_top_hits_output_schema_was_left_out`                                                                                                                                                                                                                                         |
| `structuredContent` over the result budget (G9) | partial   | A result over 48 KiB loses `structuredContent` (stashed behind `toolport_fetch_result`) unless `resultBudgets.odh` is `0` or larger (G9).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | `shaping.rs` `shape_result`, `DEFAULT_BUDGET_BYTES`; e2e `oversized_results_are_shaped_and_lose_structured_content_unless_the_budget_is_raised`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `resource_link` and `resources/read` (G6)       | supported | The link block is forwarded untouched, and a result containing it is never shaped (shaping needs all-text blocks). `resources/read` works for URIs the server lists, for URIs a listed resource template covers, and for a URI the gateway relayed in a `resource_link` of a tool result in the same session: that read goes to the server that returned the link. A listed or templated URI keeps its owner whatever a link says, and the first server to link a URI keeps it while it is remembered. A session remembers at most 256 links (the oldest goes first; a URI over 2048 bytes is not remembered), forgets them when it ends or the client initializes again, and a client scoped away from the server cannot read its links. A URI that no link and no listing names is refused as before ("no server owns resource", URI sanitized in the message) and never reaches the server. | `resource_links.rs`; `router.rs` `read_resource_from`; `toolport-gateway.rs` `SessionTables::remember_links` and the `resources/read` arm; e2e `a_link_to_an_unlisted_uri_is_read_from_the_server_that_returned_it`, `a_link_returned_through_toolport_call_tool_is_readable_too`, `a_link_is_readable_only_in_the_session_that_was_given_it`, `resource_links_pass_through_and_listed_or_templated_uris_can_be_read`; lib `resource_links::tests`, `router::tests::a_uri_no_server_lists_is_read_from_the_server_named_for_it`; `toolport-gateway` bin `a_relayed_resource_link_makes_its_uri_readable_from_the_server_that_returned_it`, `a_resource_link_does_not_take_a_listed_uri_from_the_server_that_lists_it`, `a_remembered_link_is_readable_only_in_its_own_session_and_scope`, `a_remembered_link_is_refused_once_its_server_is_gone` |
| Progress `message` notifications                | supported | Needs a client `progressToken`. The gateway sends the server its own minted token and restores the client's on every relayed `notifications/progress`, `message` and `total` included. Without a token nothing is requested. Works through `toolport_call_tool` too. Progress also re-arms the call deadline (row below).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | `toolport-gateway.rs:10050-10161`; e2e `progress_messages_reach_the_client_only_when_it_sent_a_token`, `in_lazy_mode_...`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Progress re-arms the deadline (G3)              | supported | For a stdio server, a `notifications/progress` for the call's own `progressToken` moves the deadline to now plus `requestTimeoutMs`, never past the absolute cap `maxRequestTimeoutMs` (default 1 h, B3). Only a call whose client sent a `progressToken` can be extended; a call with no progress times out at `requestTimeoutMs` with the unchanged text. Remote HTTP/SSE servers keep a fixed deadline.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | `downstream.rs:3742-3752`; e2e `progress_keeps_a_call_alive_past_request_timeout_ms`, `the_absolute_cap_still_ends_a_call_that_keeps_reporting_progress`; lib `progress_for_the_calls_token_re_arms_the_deadline_up_to_the_cap`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Elicitation wait not charged (G2)               | supported | The time the gateway spends waiting for the client to answer a server-initiated request (the human, for `elicitation/create`) is added back to both the call deadline and the cap. The wait keeps its own 120 s cap. A call that asks nothing keeps today's deadline.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `downstream.rs:3760-3766`; `toolport-gateway.rs:14066`; e2e `a_slow_elicitation_answer_is_not_charged_to_the_call_deadline`; lib `the_wait_for_a_server_request_answer_is_not_charged_to_the_deadline`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Adapter ceiling per server (G5)                 | supported | The stdio adapter no longer cuts every request at 600 s. `tools/call`, `resources/read` and `prompts/get` get the longest ceiling any configured server can hold (`requestTimeoutMs`; for a stdio server also the cap) plus 60 s, never less than 600 s. Every other request keeps 600 s. The registry is read when the call is sent, so an edit applies to the next call.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | `stdio_adapter.rs:57-60,82-97,525-532`; lib `the_call_ceiling_is_derived_from_the_servers_deadline_and_cap`, `only_downstream_calls_get_the_derived_ceiling`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Tool names                                      | supported | Full discovery lists `odh__odoo_search_read`; the client composes `mcp__toolport__odh__odoo_search_read` from the `toolport` entry key. The server receives the original name `odoo_search_read`. The composed name must fit 64 characters: the prefix `mcp__toolport__odh__` takes 20, leaving 44 for the tool. Lazy discovery lists no `odh__*` names; the model finds them with `toolport_search_tools` and calls `toolport_call_tool {name: "odh__...", arguments}`.                                                                                                                                                                                                                                                                                                                                                                                                                       | `plus/import_mcpm/ids.rs:5-15`; e2e `tool_names_carry_the_server_prefix_and_keep_the_output_schema`, `in_lazy_mode_the_odh_tools_are_reached_through_toolport_call_tool`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Server `instructions` (G7)                      | supported | Per-server `forwardInstructions`, default off. On: the server's `instructions` (from `initialize` or `server/discover`) follow the profile, else registry, else built-in text as `## Instructions from server "<name>"` and the text, one section per server in scope, registry order. At most 4096 characters per server, cut with a one-line note. Control characters are stripped, forged gateway markers neutralized, env secrets redacted; text that trips the injection scan is left out unless content defense is off. Off: the text is unchanged (B5, B6).                                                                                                                                                                                                                                                                                                                             | `handshake.rs`; `toolport-gateway.rs` `forwarded_instruction_sections`; e2e `with_forward_instructions_the_server_text_follows_the_profile_text_under_a_heading`, `forwarded_instructions_are_cut_at_the_cap`, `with_forward_instructions_off_the_gateway_text_is_unchanged`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| MCP elicitation                                 | partial   | Legacy `elicitation/create` from the server is forwarded to a client that declared `elicitation` (form mode; URL mode is brokered separately) and the answer is returned; the message names the asking server. A client that did not declare it gets `-32601 upstream client does not support elicitation/create` back to the server, and the call fails. A modern-era client that declares the capability gets MRTR `input_required` instead. The wait for the human is not charged to `requestTimeoutMs` and is capped at 120 s (G2, above).                                                                                                                                                                                                                                                                                                                                                 | `toolport-gateway.rs:14381-14471,14159,14066`; `downstream.rs:3753-3793`; e2e `a_server_elicitation_is_forwarded_to_a_client_that_declared_the_capability`, `a_server_elicitation_is_refused_when_the_client_never_declared_the_capability`, `a_slow_elicitation_answer_is_not_charged_to_the_call_deadline`; MRTR in `tests/spec_conformance.rs:651-706`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Timed-out write across a breaker trip           | supported | After 3 consecutive timeouts the circuit opens for 20 s. The first call after the cooldown is the probe: if it also times out the gateway re-spawns the server but never sends a `tools/call` again, so a slow write reaches the server once (the old child may still finish it). The caller gets the original `timed out waiting for 'tools/call' response`; the next call goes to the fresh child, and one more failure opens the circuit again. Reads, prompts and handshakes are still replayed once on the re-spawned server.                                                                                                                                                                                                                                                                                                                                                             | `router.rs:586-600,1576-1730,1815`; lib `a_probe_that_times_out_respawns_the_server_but_never_replays_the_tool_call`, `a_fresh_server_that_also_times_out_trips_the_circuit_after_one_call`, `a_probe_that_times_out_still_replays_a_resource_read_on_the_respawned_server`; e2e `a_timed_out_write_runs_once_even_when_the_breaker_probe_respawns_the_server`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `toolportctl` and `TOOLPORT_REGISTRY`           | supported | `toolportctl` reads the file `TOOLPORT_REGISTRY` names, the same one the gateway loads, and `<data dir>/registry.json` when it is unset (A1, A2).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | `plus/ctl/commands.rs:20`; `plus/registry_ro.rs:20,27`; lib `ctl_reads_the_file_toolport_registry_names_like_the_gateway`; e2e `toolportctl_and_the_gateway_read_the_same_file_under_toolport_registry`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| First connect of `uv run`                       | supported | `uv run ...` and `uv tool run ...` are download launchers: the first `initialize` waits up to 120 s, so a cold `uv run` that syncs its environment connects on the first try. Other `uv` subcommands (`sync`, `pip`, `tool install`) keep 10 s. `initializeTimeoutMs` still overrides. A server added in the desktop app with such a command is now also pre-warmed in the background, like `npx`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `downstream.rs:3264`; lib `uv_run_and_uv_tool_run_are_download_launchers_and_get_the_long_connect_budget`; e2e `a_cold_uv_run_that_needs_over_ten_seconds_to_answer_initialize_still_connects`, `initialize_timeout_ms_still_overrides_the_uv_run_launcher_budget`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

Result post-processing that applies to every `odh` result: brand-spoof neutralization (always),
content-defense scan (on by default; labels or, if `blockOnInjection`, withholds a hit), PII
pseudonymization (off by default), then shaping (`toolport-gateway.rs:5530-5600`). The benign
synthetic results in the e2e tests come through byte-identical.

## B3. Per-call deadlines

| Question                       | Answer                                                                                                                                                                                                                                                                                                                               | Evidence                                                                                                                                                                                                                                                                                                |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Default                        | 30 s per downstream request (`STDIO_READ_TIMEOUT`)                                                                                                                                                                                                                                                                                   | `downstream.rs:800,3538`                                                                                                                                                                                                                                                                                |
| Setting                        | per-server `requestTimeoutMs`, 1 ms to 24 h, widens only live calls (the initial catalog reads keep 30 s)                                                                                                                                                                                                                            | `registry.rs:572,654`; `toolport-gateway.rs:9525-9528`                                                                                                                                                                                                                                                  |
| Does progress extend it?       | Yes, for a stdio server: a `notifications/progress` for the call's own `progressToken` moves the deadline to now plus `requestTimeoutMs`, never past the absolute cap. A call whose client sent no `progressToken` is never extended. HTTP/SSE servers keep a fixed deadline                                                         | `downstream.rs:3687-3695,3742-3752`; e2e `progress_keeps_a_call_alive_past_request_timeout_ms` (a 3.6 s call reporting progress every 0.3 s finishes under `requestTimeoutMs` 1.5 s)                                                                                                                    |
| Absolute cap                   | per-server `maxRequestTimeoutMs`, 1 ms to 24 h, default 1 h; details below the table                                                                                                                                                                                                                                                 | `registry.rs:663,696`; `toolport-gateway.rs:9528`; `downstream.rs:3717-3725`; e2e `the_absolute_cap_still_ends_a_call_that_keeps_reporting_progress`                                                                                                                                                    |
| What the client sees on expiry | A normal result with `isError: true` and, as its first content block, `timed out waiting for 'tools/call' response`, not a JSON-RPC error. Same text when no progress arrived or progress stopped; only the cap adds a sentence                                                                                                      | e2e `a_call_without_progress_fails_at_request_timeout_ms_with_the_unchanged_text`; lib `without_a_cap_progress_never_re_arms_the_deadline`                                                                                                                                                              |
| A 40 s call                    | Fails under the default 30 s unless the server reports progress. Scaled down: with `requestTimeoutMs` 1.5 s, a 0.4 s call succeeds, a 4 s call without progress fails after about 1.5 s, a 3.6 s call reporting progress succeeds                                                                                                    | e2e `a_call_without_progress_fails_at_request_timeout_ms_with_the_unchanged_text`, `progress_keeps_a_call_alive_past_request_timeout_ms`                                                                                                                                                                |
| Multi-minute calls             | A tool that reports progress needs no setting (the cap allows 1 h); a silent one needs `requestTimeoutMs` high enough. The stdio adapter derives its ceiling from the servers (never below 600 s), so `requestTimeoutMs` above 600000 applies (G5)                                                                                   | `stdio_adapter.rs:57-60,82-97,525-532`; lib `the_call_ceiling_is_derived_from_the_servers_deadline_and_cap`                                                                                                                                                                                             |
| Human wait in elicitation      | Not charged to the deadline or the cap (G2). The wait for the client's answer keeps its own 120 s cap                                                                                                                                                                                                                                | `downstream.rs:3760-3766`; e2e `a_slow_elicitation_answer_is_not_charged_to_the_call_deadline`                                                                                                                                                                                                          |
| Failure side effects           | Each timeout counts toward the circuit breaker: 3 consecutive health failures open it for 20 s, calls fail fast meanwhile. The first call after the cooldown is a probe; if it fails the gateway re-spawns the child but does not replay a `tools/call` on it: the caller gets the error and the next call uses the fresh child (B2) | `router.rs:541-543,1576-1700`; e2e `a_progress_kept_call_does_not_count_toward_the_breaker_and_a_timeout_still_does`, `a_call_ended_by_the_cap_counts_toward_the_breaker`                                                                                                                               |
| Concurrency                    | Calls to one server serialize on its stdio pipe; a long call delays the next                                                                                                                                                                                                                                                         | `router.rs:1713-1716`; e2e `two_calls_to_the_same_server_run_one_after_the_other`                                                                                                                                                                                                                       |
| First connect                  | `initialize` gets 120 s for `uv run`, `uv tool run`, `uvx` and `npx` (download or sync launchers), 10 s for other commands. Per-server `initializeTimeoutMs` widens or narrows the first `initialize` only                                                                                                                           | `downstream.rs:805,3239-3276`; `toolport-gateway.rs:9418`; lib `uv_run_and_uv_tool_run_are_download_launchers_and_get_the_long_connect_budget`; e2e `a_cold_uv_run_that_needs_over_ten_seconds_to_answer_initialize_still_connects`, `initialize_timeout_ms_still_overrides_the_uv_run_launcher_budget` |

**The absolute cap.** `maxRequestTimeoutMs` is an optional per-server field next to `requestTimeoutMs`
(`registry.rs:663`): 1 ms to 24 h, unset means 1 h, and it is left out of `registry.json` while unset, so
an existing registry stays byte-identical. A value below `requestTimeoutMs` is raised to it, so a server
that sets only `requestTimeoutMs` above 1 h behaves as before. The cap bounds the whole call however much
progress arrives; the time spent waiting for the human answering a server-initiated request is not counted
toward it. It applies to stdio servers (the deadline of an HTTP/SSE server is fixed). A call that reaches
it fails with `timed out waiting for 'tools/call' response: the call reached its <cap> absolute cap
(maxRequestTimeoutMs) while the server was still reporting progress` (same `isError` result as any
timeout), and it counts toward the circuit breaker like any other timeout; a call kept alive by progress
that finishes is a success and clears the failure streak. The adapter ceiling (G5) is derived from this
value and `requestTimeoutMs`.

## B4. Spawn screening

`screen_spawn_command` (`downstream.rs:2357`) refuses wrapper programs (`sudo`, `time`, `flock`),
interpreter-eval forms (`npx -c`, `node -e`) and privileged container flags (`--privileged`,
`--cap-add`, `--device`, host namespaces). `uv` is not on the deny list, so
`uv run --directory <repo>/mcp-server odh-mcp` passes. Where the check runs:

| Path                           | Where                                                                                                                                                                                            | Test                                                                                                                                                                                    |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| mcpm import (cutover)          | `import_mcpm::screen_entry` (`launch.rs:51`), called from `prepare_launch` (`run.rs:313`)                                                                                                        | lib `import_path_maps_and_accepts_the_uv_run_directory_form` (plus the existing `all_twenty_launch_specs_pass_spawn_screening`, which has `uv run --directory`)                         |
| Registry add / catalog install | `registry_controller::apply_add_server` does not screen; the entry is stored unchanged. Screening happens when the args are resolved with a launch binding (`launch_inputs.rs:153`) and at spawn | lib `registry_add_path_stores_the_form_unchanged_and_launch_resolution_accepts_it`                                                                                                      |
| Gateway launch                 | `StdioTransport::spawn_inner` screens command and env before every spawn (`downstream.rs:3369-3370`)                                                                                             | e2e `the_uv_run_directory_form_launches_through_the_gateway` (a fake `uv` on the registry command path receives exactly `run --directory <repo>/mcp-server odh-mcp` and execs the mock) |

## B5. Instructions

`instructions` is a field on a profile in `registry.json` (`registry.rs:815`); `gatewayInstructions` is
the registry-wide default (`registry.rs:1115`). `configured_instructions` picks the connection's
profile text, then the default (`registry.rs:2535`); `server_instructions` falls back to the built-in
text when neither is set and omits the field when the text is empty
(`toolport-gateway.rs:1386`). There is no CLI or UI setter; it is a hand edit of the file
(`docs/configuration.md`, "Server instructions per profile"). No length limit is applied: a 2000
character profile text is returned from `initialize` unchanged (e2e
`a_profile_instruction_of_two_thousand_characters_is_sent_unchanged`). For the cutover, put the
ODH text on profile `claude-code` (the profile the Claude Code entry is scoped to).

A downstream server's own `instructions` are forwarded only when that server is set to
`forwardInstructions` (B6). They come after the text above, never before it and never in place of it.

## B6. Handshake switches

Two per-server fields on a registry entry change what crosses a downstream server's handshake. Both are
booleans, default off, and omitted from `registry.json` while off, so a registry that does not use them is
byte-identical to before (`registry::tests::the_handshake_switches_default_off_stay_out_of_the_file_and_round_trip`).

| Field                       | On                                                                                                                                      |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `declareClientCapabilities` | The server's legacy `initialize` declares the `roots`, `sampling` and `elicitation` its client declared, instead of `{}`                |
| `forwardInstructions`       | The server's `instructions` go into the gateway's `instructions`, under a heading with the server name, after the profile/registry text |

**Setting them.** Edit `registry.json`, or use any of the existing edit paths; each validates the value as a
boolean and keeps every field it does not name:

- `toolportctl server edit <id|name> --declare-client-capabilities on|off --forward-instructions on|off`
  (`on`, `true`, `yes`, `1`, `off`, `false`, `no`, `0`; anything else is a usage error, exit 2). `server new`
  does not take them; edit the server afterwards.
- selfmcp `servers_update_config` and `servers_install`: `declareClientCapabilities` and `forwardInstructions`
  in the config or patch, JSON `true` or `false` only (`invalid_arguments` otherwise); `servers_get` shows both.
- The desktop IPC `update_server` takes the whole entry. The server dialog keeps the fields it has no control for
  (these two, `maxRequestTimeoutMs`, disabled tools) when an edit is saved; it has no control for them yet.
- `toolportctl server info` shows both (`--json` always; the human form adds a `Declares:` or `Forwards:` line
  only while on). A re-run of `import-mcpm` keeps what was set.

**Client capabilities.** The gateway builds its router when it starts, before any client has said what it
supports, so a server set to `declareClientCapabilities` first connects with `{}`. When a client's
`initialize` arrives, the gateway notes its `roots`, `sampling` and `elicitation` (what it can relay; `experimental`,
`extensions` and `tasks` are not declared downstream) and re-spawns each such server whose declaration is
now out of date. The client's own `initialize` waits for that for at most 10 s; a server that is still
starting finishes in the background. The declaration is the union over every client the gateway has
served and only grows, because one gateway shares its downstream connections between clients; each
request is still answered for the client that made it, so a client that did not declare `elicitation` is
refused a server's `elicitation/create` as before (e2e
`the_declaration_is_what_the_clients_of_the_shared_gateway_declared_and_each_answers_for_itself`).
A modern-era (2026-07-28) client declares per request, which the gateway also notes. A server pooled per
client root (`${ROOT}`) is not re-spawned proactively. The in-process stdio gateway
(`TOOLPORT_GATEWAY_TOPOLOGY=legacy`) behaves the same way.

**Instructions.** Each server in the connection's scope that is set to `forwardInstructions` and has any adds
one section, in registry order, after the profile text (else `gatewayInstructions`, else the built-in text); with
no text of its own the sections stand alone. The heading is `## Instructions from server "<name>"`, with the
name on one line, quotes replaced and at most 64 characters.

- **Cap:** 4096 characters per server (`handshake::MAX_FORWARDED_INSTRUCTIONS_CHARS`), cut at a character
  boundary with `[Toolport: cut at 4096 characters]` after it. There is no total cap beyond that.
- **Secrets-free:** for a stdio server, the values of its env secrets and resolved launch inputs are redacted
  from the text when the server connects. The text is also stripped of control characters other than newline and tab, and
  forged gateway markers such as `[Toolport advisor: ...]` are neutralized.
- **Injection scan:** with content defense on (the default), text that matches an injection signature is left
  out of the gateway's instructions and the gateway log says why; with it off the text is forwarded.
- **When:** read from the live connection, so a server that is not connected yet contributes nothing. An
  `initialize` or `server/discover` waits up to 10 s for the first build when a forwarding server is in scope.
  Instructions are captured on every (re)connect. Toggling `forwardInstructions` does not restart any server.

## Gap list

Size: XS under 50 lines, S 50-300, M 300-800, L more. Each row is a proposal, not a decision. G1, G2, G3, G4, G5, G6, G7, G8, G10 and G11 are done (MIG-ODH-2, MIG-ODH-3,
MIG-ODH-4, MIG-ODH-5) and now live in the support table (B2).

| #   | Gap                                                                                                                                                                       | Proposal                                                                                                                                         | Size |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ---- |
| G9  | A result over 48 KiB loses `structuredContent`; a client that validates it against a declared `outputSchema` (a SHOULD in the spec; not tested here) would see it missing | Keep `structuredContent` when the tool declares an `outputSchema`, or shape only `content`. Today: `resultBudgets: {"odh": 0}` or a larger value | S    |
| G12 | Calls to one server serialize, so a multi-minute ODH call blocks the next ODH call                                                                                        | Document first; a per-server concurrency setting only if the downstream supports it                                                              | M    |

G12 is a property of one stdio pipe, not necessarily a defect.

## Test index

E2E (`src-tauri/tests/odh_integration.rs`, mock profile in `src-tauri/src/bin/mock-mcp-server.rs`):
`the_gateway_advertises_what_it_serves_and_declares_nothing_downstream`,
`the_gateway_answers_every_known_revision_with_that_revision`,
`tool_names_carry_the_server_prefix_and_keep_the_output_schema`,
`in_lazy_mode_the_odh_tools_are_reached_through_toolport_call_tool`,
`lazy_search_gives_the_top_hit_its_output_schema_and_the_menu_entries_none`,
`resource_links_pass_through_and_listed_or_templated_uris_can_be_read`,
`a_link_to_an_unlisted_uri_is_read_from_the_server_that_returned_it`,
`a_link_returned_through_toolport_call_tool_is_readable_too`,
`a_link_is_readable_only_in_the_session_that_was_given_it`,
`progress_messages_reach_the_client_only_when_it_sent_a_token`,
`a_server_elicitation_is_forwarded_to_a_client_that_declared_the_capability`,
`a_server_elicitation_is_refused_when_the_client_never_declared_the_capability`,
`a_slow_elicitation_answer_is_not_charged_to_the_call_deadline`,
`a_call_without_progress_fails_at_request_timeout_ms_with_the_unchanged_text`,
`progress_keeps_a_call_alive_past_request_timeout_ms`,
`the_absolute_cap_still_ends_a_call_that_keeps_reporting_progress`,
`a_progress_kept_call_does_not_count_toward_the_breaker_and_a_timeout_still_does`,
`a_call_ended_by_the_cap_counts_toward_the_breaker`,
`two_calls_to_the_same_server_run_one_after_the_other`,
`oversized_results_are_shaped_and_lose_structured_content_unless_the_budget_is_raised`,
`a_profile_instruction_of_two_thousand_characters_is_sent_unchanged`,
`the_uv_run_directory_form_launches_through_the_gateway`,
`a_timed_out_write_runs_once_even_when_the_breaker_probe_respawns_the_server`,
`toolportctl_and_the_gateway_read_the_same_file_under_toolport_registry`,
`a_cold_uv_run_that_needs_over_ten_seconds_to_answer_initialize_still_connects`,
`initialize_timeout_ms_still_overrides_the_uv_run_launcher_budget`,
`with_declare_client_capabilities_the_downstream_initialize_carries_exactly_the_clients`,
`with_declare_client_capabilities_a_server_that_gates_elicitation_asks_and_the_answer_comes_back`,
`with_the_opt_in_off_every_downstream_initialize_stays_empty_and_a_gating_server_never_asks`,
`with_declare_client_capabilities_a_client_that_declared_nothing_adds_nothing`,
`the_declaration_is_what_the_clients_of_the_shared_gateway_declared_and_each_answers_for_itself`,
`the_in_process_stdio_gateway_declares_the_clients_capabilities_too`,
`a_modern_client_that_declares_elicitation_per_request_reaches_a_gating_legacy_server`,
`with_forward_instructions_the_server_text_follows_the_profile_text_under_a_heading`,
`forwarded_instructions_follow_the_built_in_text_when_no_profile_text_is_set`,
`forwarded_instructions_are_cut_at_the_cap`,
`a_profile_that_sends_no_text_still_carries_the_forwarded_instructions`,
`with_forward_instructions_off_the_gateway_text_is_unchanged`.

Lib (`odh_tests`): `import_path_maps_and_accepts_the_uv_run_directory_form`,
`registry_add_path_stores_the_form_unchanged_and_launch_resolution_accepts_it`,
`uv_run_and_uv_tool_run_are_download_launchers_and_get_the_long_connect_budget`,
`the_documented_registry_and_client_entry_are_what_the_cutover_writes`,
`ctl_server_info_names_the_launch_line_and_env_keys_but_never_env_values`,
`the_registry_file_keeps_non_secret_env_values_and_vaults_the_secret_ones`,
`ctl_reads_the_file_toolport_registry_names_like_the_gateway`.

Lib (deadline rows, in their own modules): `downstream::tests::progress_for_the_calls_token_re_arms_the_deadline_up_to_the_cap`,
`downstream::tests::progress_for_another_token_leaves_the_deadline_alone`,
`downstream::tests::without_a_cap_progress_never_re_arms_the_deadline`,
`downstream::tests::the_cap_ends_a_call_that_keeps_reporting_progress`,
`downstream::tests::the_wait_for_a_server_request_answer_is_not_charged_to_the_deadline`,
`stdio_adapter::tests::the_call_ceiling_is_derived_from_the_servers_deadline_and_cap`,
`stdio_adapter::tests::only_downstream_calls_get_the_derived_ceiling`,
`registry::tests::max_request_timeout_is_optional_and_never_undercuts_the_call_deadline`,
`registry::tests::only_a_stdio_server_can_reach_its_progress_cap`.

Lib (resource link rows, in their own modules): the whole of `resource_links::tests`,
`router::tests::a_uri_no_server_lists_is_read_from_the_server_named_for_it`, the `toolport-gateway` bin tests
`a_relayed_resource_link_makes_its_uri_readable_from_the_server_that_returned_it`,
`a_resource_link_does_not_take_a_listed_uri_from_the_server_that_lists_it`,
`a_remembered_link_is_readable_only_in_its_own_session_and_scope`,
`a_remembered_link_is_refused_once_its_server_is_gone`.

Lib (lazy search rows, in the `toolport-gateway` bin tests): `the_top_hit_carries_its_output_schema_and_menu_entries_do_not`,
`a_top_hit_without_a_usable_output_schema_gets_neither_the_field_nor_the_flag`,
`an_output_schema_is_kept_up_to_its_byte_cap_and_left_out_flagged_past_it`,
`a_top_hits_output_schema_cannot_speak_as_the_gateway`,
`search_text_says_when_a_top_hits_output_schema_was_left_out`.

Lib (handshake rows, in their own modules): the whole of `handshake::tests`;
`registry::tests::the_handshake_switches_default_off_stay_out_of_the_file_and_round_trip`,
`downstream::tests::a_legacy_initialize_declares_no_capabilities_unless_the_caller_passes_some`,
`downstream::tests::the_servers_instructions_are_kept_from_either_era_and_blank_ones_are_not`,
`downstream::tests::instructions_can_be_redacted_once`,
`router::tests::a_server_declaring_the_client_is_re_spawned_when_the_declaration_has_grown`,
`router::tests::a_server_that_cannot_be_re_spawned_keeps_its_connection`,
the `toolport-gateway` bin tests `forwarded_instructions_come_from_in_scope_servers_set_to_forward_after_the_profile_text`,
`forwarded_instructions_follow_the_built_in_text_and_stand_alone_when_the_profile_omits_its_own`,
`without_the_switch_or_a_text_the_gateway_text_is_unchanged`,
`a_server_whose_instructions_carry_an_injection_is_left_out_unless_defense_is_off`,
`forward_instructions_is_not_router_relevant_but_declare_client_capabilities_is`;
the edit paths: `registry_controller::tests::a_field_edit_sets_the_handshake_switches_only_when_it_carries_them`,
`plus::servers::tests::a_switch_reads_on_and_off_spellings_and_nothing_else`,
`plus::ctl::server_tests::the_handshake_switches_are_set_through_server_edit_and_shown_by_info`,
`plus::selfmcp::wired_tests::server_mutations_follow_their_tiers`,
`plus::import_mcpm::run_tests::rerun_keeps_the_gateway_settings_a_user_made_on_an_imported_server`;
frontend `src/components/ServerDialog.test.tsx` (`keeps the gateway settings it has no control for when editing a server`).
