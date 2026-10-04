# Toolport+ extensions

Toolport+ is a fork of Toolport. Everything the fork adds lives under `src-tauri/src/plus/` (Rust) and `src/plus/` (React), so upstream merges rarely conflict. The desktop app reaches the Rust side through one generic IPC command, `plus_invoke`, and the `toolportctl` binary reaches it through a command table.

## Module map

| Module (`src-tauri/src/plus/`) | What it does                                                                                                                                               |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`                       | Handler table (`plus.*` names) and `dispatch`; the only IPC surface (`plus_invoke`).                                                                       |
| `ctl/`                         | `toolportctl` command line: parsing, the `COMMANDS` table, JSON envelope output, per-area command files.                                                   |
| `direct/`                      | Opt-in direct client entries: ownership record, `direct run` launcher, core of `client direct`.                                                            |
| `auth/`                        | Expired-login detection: stdio and remote auth probes, status cache, gateway state, Google OAuth refresh, status surfaces (statusline, SessionStart hook). |
| `context/`                     | Context engine: layered rules, `CLAUDE.local.md` deploy, settings union, legacy MCP dedupe, shims, "what loads" viewer, folder profiles.                   |
| `compression/`                 | Compression policy, provider health, shims, launch plan and the token-savings ledger.                                                                      |
| `skills/`                      | SKILL.md parser, lockfile, lint, collision backups, per-client transpilers (agents, styles, assets), skill taps (add, update, search, install).            |
| `sync/`                        | Encrypted cross-machine sync (Fernet bundle, KDF, git transport, origins).                                                                                 |
| `import_mcpm/`                 | Pure mapping from an mcpm config root to registry servers, name map, tool-reference rewrite.                                                               |
| `selfmcp/`                     | Self-management MCP server: tool and resource catalog, confirm-tier enforcement, backends onto skills, registry and client sync.                           |
| `update/`                      | Update checks and apply for registry servers (git ff-only, release binaries, npx/uvx pins).                                                                |
| `obs/`                         | Claude Code transcript indexer, loopback OTLP receiver and its settings switch, monitor database import.                                                   |
| `cc/`                          | Claude Code plugin list and update.                                                                                                                        |
| `council/`                     | Council as a downstream MCP server definition.                                                                                                             |
| `jsonfs.rs`                    | Tolerant JSON file read shared by the modules above.                                                                                                       |
| `testutil.rs`                  | Test-only fixtures (see Test conventions).                                                                                                                 |

Frontend (`src/plus/`): `api.ts` (`plusInvoke` wrapper), `AuthRows` and `AuthNotifier`, `FolderProfiles`, `WhatLoads`, with fixtures under `src/plus/fixtures/`. New IPC commands must be registered in `src/plus/fixtures/plusInvoke.ts`, which `src/test/browser-fixture.tsx` serves.

## toolportctl commands

Global flag `--json` prints one envelope (`schemaVersion`, `command`, `data`). Exit codes: 0 ok, 1 error, 2 usage. The table below mirrors `COMMANDS` in `ctl/mod.rs`; group rows (no handler) print help.

| Command                                                                 | Purpose                                                            |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `status`                                                                | Registry, profile, secrets backend and gateway state               |
| `doctor`                                                                | Read-only health checks                                            |
| `server ls / search / install / uninstall / info / new / edit`          | Catalog and registry server management                             |
| `inspect`, `profile inspect`                                            | List tools of a server or of a whole profile (connects live)       |
| `profile ls / create / edit / rm`                                       | List, create, edit and remove profiles; `rm` cleans clients        |
| `client ls / sync / edit / import`                                      | Detect clients; sync; set a client's profile; import entries       |
| `client direct add / rm / ls`                                           | Opt-in: give one client a direct entry for one stdio server        |
| `direct run`                                                            | Stdio launcher a direct client entry starts (not run by hand)      |
| `auth statusline / hook / probe / login`                                | Auth-health JSON; probe now (`--server`, `--force`); sign in again |
| `secret set / get / rm`                                                 | Server secrets (stdin or `--value-env`; `get --reveal` prints)     |
| `context loads / folders / checkpoint-status / plan / apply / sync`     | What a session loads, folder profiles, checkpoint, context deploy  |
| `context init / status / client / profile / disable`                    | Scaffold layers and launch profiles, show state                    |
| `compression status / presets / run / verify / ledger / proxy / update` | Compression policy and launch                                      |
| `import mcpm / rename-refs`                                             | Import an mcpm root; rewrite tool references                       |
| `council`                                                               | Council server install, uninstall, doctor, tools                   |
| `mcp`                                                                   | Self-management server: install, uninstall, doctor, tools          |
| `skills init/add/ls/lint/audit/bundle/unbundle/sync/diff`               | Skills repo init, add, list, lint, audit, bundle, sync, diff       |
| `skills status / clean / uninstall / resolve`                           | Lock vs outputs, remove managed outputs, uninstall, collisions     |
| `skills tap add/ls/remove/update, search, install`                      | Skill taps: register, list, pull, search, install (`--dry-run`)    |
| `agents add/ls/lint/audit/diff/status/clean/uninstall/sync`             | Agents: template, list, lint, audit, drift, sync, remove           |
| `styles add/ls/lint/diff/status/sync/apply/remove/clean`                | Output styles: template, list, lint, drift, sync, apply, remove    |
| `sync`                                                                  | Encrypted sync (init, push, pull, diff, status, reset, ...)        |
| `cc`                                                                    | Claude Code plugins: list, update                                  |
| `update`                                                                | Server updates (`--check`, `--apply`, `--init`, `--dry-run`)       |
| `usage`                                                                 | Token and MCP usage from Claude Code transcripts                   |
| `obs otel enable / disable / status`                                    | Loopback OTLP receiver and Claude Code telemetry env in settings   |

## OTel receiver

`toolportctl obs otel enable` turns on Claude Code telemetry and a loopback OTLP/HTTP-JSON receiver that stores it next to the transcript index. It is off until you run it.

- `enable [--port <n>] [--home <dir>] [--dry-run]` writes `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_METRICS_EXPORTER=otlp`, `OTEL_LOGS_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` and `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port>` into the `env` block of `~/.claude/settings.json` and records the choice in `<data dir>/obs/otel.json` (`{"enabled": true, "port": 4318}`). Other keys are kept; a key that already holds a different value is a conflict (exit 1, nothing written). Prompt, tool-content and e-mail logging is never switched on, and those attributes are dropped when received.
- `disable [--home <dir>] [--dry-run]` removes only what `enable` wrote (and a settings file or `env` block it created), keeps a key you changed since, and stops the receiver.
- `status [--home <dir>]` reports the receiver (`disabled`, `listening`, `stopped`, `port-in-use`), the settings (`configured`, `partial`, `missing`, `unreadable`) and how many events are stored.

All three accept `--json` and run the same code as the `plus.obs.otel.enable`, `plus.obs.otel.disable` and `plus.obs.otel.status` IPC handlers (`home`, `port`, `dryRun` arguments).

The receiver runs inside the `toolport-gateway` process, which lives as long as a Claude Code session that uses Toolport. The gateway re-reads `obs/otel.json` every two seconds, so `enable` and `disable` take effect without a restart, and a second gateway on the same machine waits and takes over if the first one exits. If the port is taken by another program the gateway logs it once and retries every ten seconds; `status` shows `port-in-use`. Without a running gateway nothing receives, and Claude Code drops the export.

It binds `127.0.0.1` only and answers `POST /v1/metrics` and `POST /v1/logs` (`application/json`, optionally gzip, at most 4 MiB), `GET /healthz`, and nothing else. A request whose `Host` is not a loopback name is refused. Success returns `{"partialSuccess":{}}`; bad input returns 400, 404, 405, 411, 413, 415 or 431 with a `{"code","message"}` body. Request content is never logged.

`toolportctl usage` and `plus.obs.summary` merge the stored `api_request` events with the transcripts: a request that appears in both (same request id, or an id-less match on the same model and token counts within ten minutes) is counted once, and requests only seen through OTel are added to the totals. The summary reports the split under `sources`.

## Self-management MCP server

`toolport-plus-self` is the `toolport-selfmcp` binary registered as a stdio server: 80 tools and 11 `mcpm://` resources for skills, agents, styles, compression, servers, clients and sync. `import mcpm`, `toolportctl mcp install` and `plus.selfmcp.ensure` register it and switch it on in the active (default) profile and in the profile of every connected client (`clientScopes`), so every client connected through the gateway sees its tools. A profile created later (a new client) gets it the next time one of those runs. The binary must sit next to `toolportctl` or in `<data dir>/bin`; `toolportctl mcp doctor` checks it.

**What it costs.** The 80 tool definitions are about 38 KB of JSON, roughly 9,300 tokens, in every client that lists all tools (full discovery, which import sets for Claude Code). A client in lazy discovery (the import default for every other client) lists none of them and pays only when it searches with `toolport_search_tools`. Exposed names are at most 46 characters (`toolport_plus_self__servers_remove_profile_tag`), which keeps `mcp__toolport__...` inside the 64 character limit of Claude Code. Tier 3 and tier 4 tools refuse unless `confirm` is true.

**Which tools preview by default.** The tap tools, `skills_install`, `client_direct_add|rm` and the lifecycle tools added after them take `dry_run` and default it to true: the call only reports what it would do until `dry_run` is passed as false. The later ones are `skills_bundle`, `skills_unbundle`, `skills_clean`, `skills_uninstall`, `skills_resolve`, `agents_clean`, `agents_uninstall`, `styles_clean` and `compression_enable`, `compression_disable`, `compression_set_provider`, `compression_use`, `compression_sync` and `compression_seal`. For the tier 3 and tier 4 ones `confirm` is needed only to apply (`dry_run=false` and `confirm=true`). Each runs the same core as its `toolportctl` command, and the read-only counterparts are `skills_diff`, `skills_audit`, `agents_diff`, `agents_audit`, `agents_status`, `styles_diff`, `styles_status` and `compression_status`. `skills_bundle` only creates a new `.zip` and never overwrites a file, `skills_unbundle` refuses a bundle that holds files outside `skills/`, `rules/` and `mcpm-skills.yaml`, and `servers_auth` starts the server through the same screened launcher as the gateway (`auth::stdio::start`). The older write tools (`skills_sync`, `agents_sync`, `styles_sync_tier1`, `styles_apply`, `styles_remove`, `clients_sync`, `sync_push`) keep their own `dry_run` without a default. Not exposed, because they drive the engine or read a caller-chosen directory: `compression disable --teardown`, `compression sync --mcpm-root`, and `compression pin`, `presets`, `proxy`, `update`, `run`, `verify` and `ledger`.

**Turning it off.**

- Everywhere: `toolportctl mcp uninstall` removes the server and records the opt-out. Import and `plus.selfmcp.ensure` never add it again; `toolportctl mcp install` does.
- In one profile: switch the server off in that profile (the profile toggle in the app). A profile Toolport already switched it on in is never switched on again by import or `install`; `toolportctl mcp install --profile <id>` switches it on there on purpose.
- One client only: set that client to lazy discovery, or scope it to a profile without the server.

**Where the choice lives.** `registry.json`, top level: `"plus": {"selfmcp": {"enabledIn": ["default", ...], "optOut": true}}`. `enabledIn` lists the profiles it was switched on in once; `optOut` is written by `mcp uninstall`. Deleting the server in the app counts as an opt-out too.

`toolportctl mcp doctor` reports the state: `enabled`, `disabled` (switched off on purpose in a profile, healthy), `opted-out` (uninstalled, healthy), `not-enabled` and `missing` (both fail with exit code 1, and `toolportctl mcp install` repairs them).

The gateway removes every `TOOLPORT_*` variable from the servers it starts. If you move the data directory with `TOOLPORT_DATA_DIR`, give the self server the same variable in its own `env`, or its tools manage the default data directory instead.

## Direct client entries (opt-in)

By default every client reaches every server through the one gateway entry. For a few servers that is the wrong shape, for example a local stdio server a single client should start itself. `toolportctl client direct add <server> --client <id>` gives that client an entry named after the server, next to the gateway entry:

```
toolportctl client direct add odoo --client claude-code --dry-run   # plan, write nothing
toolportctl client direct add odoo --client claude-code
toolportctl client direct ls [--client claude-code]
toolportctl client direct rm odoo --client claude-code
```

The entry runs `toolportctl direct run <server id>`, never the server's own command, and holds no secret. The launcher reads the server's command, arguments and environment from the registry and the secret store when the client starts it, applies the same spawn checks as the gateway, removes `TOOLPORT_*` from the child's environment, hands over stdin and stdout and gives back the server's exit status (on Unix it replaces itself with the server, so nothing sits between the client and the server). If you moved the data directory with `TOOLPORT_DATA_DIR`, that one variable is repeated in the entry's `env`; the encrypted secret file's `TOOLPORT_SECRET_KEY` never is, so a client that uses it must have it in its own environment.

**What you give up.** A direct entry bypasses the gateway. Its tools are not covered by profile tool scopes, approvals (HITL), receipts or lazy discovery, and every client with a direct entry starts its own process of the server instead of sharing one. `direct add` prints this, and so does its dry run.

**What is allowed.** Local stdio servers only. A remote (http, sse) server, a server whose sign-in Toolport manages (OAuth) and a server with a `${ROOT}` working directory only work through the gateway; `direct add` refuses them with the reason, exits with code 1 and writes nothing. An existing entry with the same name that Toolport did not write needs `--force`.

**Ownership.** The registry records every entry it wrote under `plus.directEntries.<client>.<entry>`. Because of that record:

- `client sync` leaves the entry alone, and still prunes foreign direct entries that duplicate or orphan registered servers.
- `client ls`, `status` and `doctor` report launcher entries; `client direct ls` gives each a state: `ok`, `stale` (the launcher binary moved), `customized` (edited after it was written; `rm` and `add` need `--force`), `missing` (recorded, gone from the client), `orphan` (the server was removed) or `unrecorded` (launcher-shaped but not recorded).
- `server uninstall` removes the entries in every client, and `client import` does not import them back.
- Writes use the normal client writers: a backup first, every other key and server kept, every supported format, and JSON comments kept where the format has them.

The desktop handlers are `plus.client.directAdd`, `plus.client.directRm` and `plus.client.directLs` (arguments `server`, `client`, `force`, `dryRun`); the self-management server has `client_direct_ls` (tier 1) and `client_direct_add` and `client_direct_rm` (tier 2). Like the tap tools, the two writing tools plan only unless `dry_run` is passed as false.

## Login health

`toolportctl auth probe` runs the due probes, `--force` ignores the cache and `--server <id>` limits the run to one server; it writes the same cache as the gateway due-scan. That scan runs from the registry-watch loop of every long-lived gateway mode, on its own thread, at most once a minute after a 60 second delay (`TOOLPORT_AUTH_SCAN=off` disables it), so probes never hold up requests.

`toolportctl auth login <server>` is the fix the status surfaces point at. A remote OAuth server runs the gateway browser flow; a stdio server runs `<command> <args> auth` and prints the consent URL it prints (`--no-open` leaves the browser to you). A server that signs in with an API token or client credentials gets the next step instead. A stdio server joins the probes by opting in with `"plus": {"authProbe": {"kind": "stdio"}}` in its registry entry.

The app shows the same rows under Settings as "Sign-in health" (`plus.auth.rows`). A reauth or reconsent row signs in through `plus.auth.login`, the same core as `toolportctl auth login` and it opens the browser; a retry row runs the `plus.auth.probe` route it carries; a misconfigured row only names what to check. Each fix reloads the rows. While the window is visible, `AuthNotifier` asks `plus.auth.notifications` once a minute and raises a toast for each login that just moved to needs-reauth or expiring and is still in that state. A login is announced once per six hours; what was announced lives in `auth/notified.json` next to `status.json`. There is no operating-system notification yet.

## Environment variables

| Variable                                                                                                                                                                                                                                       | Effect                                                                                                               |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `TOOLPORT_SECRET_KEY`                                                                                                                                                                                                                          | Enables the headless encrypted-file secrets backend (key is the SHA-256 of the value). Tests use a synthetic value.  |
| `TOOLPORT_SECRETS_BACKEND`                                                                                                                                                                                                                     | `login-keychain` selects the macOS login-keychain master key (also a `secrets-backend` marker file in the data dir). |
| `TOOLPORT_ALLOW_BARE_SECRET_ENV`                                                                                                                                                                                                               | Legacy: resolve secrets from bare process env names.                                                                 |
| `TOOLPORT_CORP_TOOLS_DIR`                                                                                                                                                                                                                      | Directory of a corporate tools checkout the context engine coexists with.                                            |
| `TOOLPORT_CLIENTS_ROOT`                                                                                                                                                                                                                        | Root directory holding client repositories for context deploy.                                                       |
| `TOOLPORT_AUTH_SCAN`                                                                                                                                                                                                                           | `0`, `off` or `false` turns off the gateway due-scan of login health (default on).                                   |
| `TOOLPORT_SKILL_ASSET_EXTENSIONS`                                                                                                                                                                                                              | Extra file extensions copied as skill assets.                                                                        |
| `TOOLPORT_CLAUDE_BIN`                                                                                                                                                                                                                          | Claude binary used by `cc` list/update.                                                                              |
| `TOOLPORT_DATA_DIR`, `TOOLPORT_REGISTRY`                                                                                                                                                                                                       | Data directory and registry path overrides.                                                                          |
| `TOOLPORT_LOCK_TIMEOUT_MS`                                                                                                                                                                                                                     | Lock wait (default 5 s).                                                                                             |
| `TOOLPORT_PROFILE`, `TOOLPORT_CLIENT_ID`, `TOOLPORT_ROOT`                                                                                                                                                                                      | Gateway profile, client identity and root selection.                                                                 |
| `TOOLPORT_HTTP_TOKEN`, `TOOLPORT_RESULT_BUDGET`, `TOOLPORT_NO_DIRECT_SPAWN`, `TOOLPORT_DEBUG`, `TOOLPORT_CODE_MODE`, `TOOLPORT_SEMANTIC`, `TOOLPORT_EMBED_*`, `TOOLPORT_METRICS`, `TOOLPORT_GATEWAY_TOPOLOGY`, `TOOLPORT_DAEMON_IDLE_GRACE_MS` | Upstream gateway tuning, see `docs/configuration.md`.                                                                |

Legacy `CONDUIT_*` names are still read through `brand::env_var`.

## Test conventions

- Anything touching the data dir takes `registry::data_dir_test_lock()` and sets `registry::DataDirOverride`. `plus::testutil::DataDirFx` does both, creates a fresh temp dir, and removes it on drop; wrap it (with `Deref`) to add per-area setup.
- Fixture field order matters: fields drop in declaration order, so the override must be declared before the lock guard. `DataDirFx` encodes this once; do not reorder.
- Per-thread overrides (`clients::TEST_HOME`, `selfmcp` `TEST_REPO`) must be cleared in the fixture's `Drop`, which runs before its fields drop.
- Tests that read or set process-global env (`XDG_*`, `CLAUDE_CONFIG_DIR`, `TOOLPORT_LOCK_TIMEOUT_MS`) hold `clients::env_test_lock()`, taken before the data-dir lock.
- The data-dir override is process-global. A libtest thread that does not hold the lock resolves the real data dir while another test holds it, so it cannot leave `gateway.log`, `secret-service.lock` or similar files in that test's dir; threads a test spawns itself keep seeing its override. A test that spawns its own threads and touches the data dir must therefore hold `DataDirFx`.
- Executable fixture scripts go through `write_executable` (`tests/common/exec.rs`; lib tests use `plus::testutil::exec`, integration tests include it with `#[path]`). It returns only once the file runs, because a script written while another thread forks fails with `ETXTBSY`.
- `tests/one_gateway_conformance.rs` counts gateway daemons and mock servers by this build's own binary path, so gateway tests running from another target dir or slot on the same host do not disturb it. Two runs of that suite from the _same_ target dir at the same time are unsupported.
- Secrets tests use `secrets::tests::with_isolated_vault` (isolated vault dir plus the secret-key env lock) and synthetic keys only; never real credentials.
- Never run `cargo fmt` on the tree; format only new files. Run suites with `cargo test --no-default-features --tests --no-fail-fast` and `npm run verify`.
- Use synthetic fixtures and `.invalid` hosts; no real organization or customer names.
