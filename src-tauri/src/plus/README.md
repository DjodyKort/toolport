# plus/ module index

Toolport+ additions (D-010). Full overview: `docs/toolport-plus.md`.

- `mod.rs`: `plus.*` handler table and `dispatch` behind the single `plus_invoke` IPC command.
- `ctl/`: `toolportctl` command table, parsing and JSON/human output.
- `auth/`: login-expiry probes, status cache, gateway state, status surfaces.
- `context/`: layered rules deploy, what-loads viewer, folder profiles.
- `compression/`: compression policy, launch plan, savings ledger.
- `skills/`: skill parser, lockfile, lint, transpilers; `ops` is the shared core under `toolportctl skills`, the `plus.skills.*` handlers and the selfmcp `skills_status` tool.
- `sync/`: encrypted cross-machine sync.
- `import_mcpm/`: mcpm to registry mapping and tool-reference rewrite.
- `selfmcp/`: self-management MCP server.
- `update/`: server update checks and apply.
- `obs/`: transcript indexer and local OTel sink.
- `cc/`: Claude Code plugin list and update.
- `council/`: council downstream server definition.
- `jsonfs.rs`: `read_json`, tolerant typed JSON read (None on missing or invalid).
- `testutil.rs` (test only): `DataDirFx`, data-dir lock plus override plus temp dir with the required drop order.
