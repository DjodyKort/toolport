# plus/ module index

Toolport+ additions (D-010). Full overview: `docs/toolport-plus.md`.

- `mod.rs`: `plus.*` handler table and `dispatch` behind the single `plus_invoke` IPC command.
- `ctl/`: `toolportctl` command table, parsing and JSON/human output.
- `auth/`: login-expiry probes, status cache, gateway state, status surfaces.
- `context/`: layered rules deploy, what-loads viewer, folder profiles; `manage` is the shared core under `toolportctl context init|status|client|profile|disable` and the `plus.context.*` handlers of the same names.
- `compression/`: compression policy, launch plan, savings ledger.
- `skills/`: skill parser, lockfile, lint, transpilers; `ops` is the shared core under `toolportctl skills`, the `plus.skills.*` handlers and the selfmcp `skills_status` tool. `skills/agents/` holds the agent transpilers and `manage`/`handlers`, the core under `toolportctl agents` and the `plus.agents.*` handlers (sync, clean and uninstall share `ops` and `sync_scoped` with the selfmcp `agents_sync` tool).
- `sync/`: encrypted cross-machine sync.
- `import_mcpm/`: mcpm to registry mapping and tool-reference rewrite.
- `selfmcp/`: self-management MCP server.
- `update/`: server update checks and apply.
- `obs/`: transcript indexer, loopback OTLP receiver (`receiver`, hosted by the gateway through `otel_host`), `otel_setup` (the `obs otel` enable, disable and status core) and `combine` (OTel and transcript dedupe).
- `cc/`: Claude Code plugin list and update.
- `council/`: council downstream server definition.
- `jsonfs.rs`: `read_json`, tolerant typed JSON read (None on missing or invalid).
- `testutil.rs` (test only): `DataDirFx`, data-dir lock plus override plus temp dir with the required drop order.

## Adding a `toolportctl` command group

A group is a spec table plus handlers; no parsing code.

1. Declare the flags per command in the handler's module (`ctl/flags.rs`):
   `const SYNC: Spec = Spec { flags: &[switch("--dry-run"), value("--path").alias(&["--repo"]).needs("a directory")], ..Spec::PLAIN };`.
   `switch`, `value` and `greedy` build flags; `.alias`, `.needs`, `.count`, `.nonempty` refine them. The style fields (`inline`, `unknown`, `operands`, `dashes`) pick the wording and edge cases; `Spec::NONE` is a command without arguments.
2. Write the handler: `let flags = SYNC.parse(rest)?;`, then `flags.on("--dry-run")`, `flags.one("--path")`, `flags.all(..)`, `flags.operands()`. Return `CtlError::usage`, `not_found`, `conflict` or `failed("code", msg)`.
3. Add one `cmd(&["group", "sub"], "summary", module::handler)` row per command to `COMMANDS` in `ctl/mod.rs`, plus a row for the bare group whose handler returns the group usage error. `.no_options()` is only for commands that must reject unknown `-x` tokens at the top level.
4. Pin the usage and error strings first: add argv lists to `CASES` in `ctl/usage_pins_tests.rs` and bless them with `UPDATE_GOLDEN=1 cargo test --no-default-features --lib usage_and_error_text`. Also classify the command in `tests/ctl_smoke.rs` and `tests/hardening_leak.rs`.
