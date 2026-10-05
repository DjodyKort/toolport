# MIG-AUTH-9 — Re-auth hint matches the probe kind

## Commit

`352d63580725a439a4da0643523c68029bd180ef` on `mig/MIG-AUTH-9` (local only, not pushed):

```
[FIX] auth: match re-auth hint to probe kind

fix_action always pointed every NeedsReauth/Revoked server at
`toolportctl auth login`, but login::plan refuses that for API-token
(Http-kind) probes like stitch, telling the user to run `secret set`
instead. Track a ProbeHintKind (OAuth/ApiToken) alongside each probe's
cached entry and have fix_action pick the matching hint, so the probe
table, statusline and SessionStart hook all stop pointing at a dead
end for API-token servers.
```

8 files changed, 214 insertions(+), 24 deletions(-):
`auth/cache.rs`, `auth/mod.rs`, `auth/notify_tests.rs`, `auth/prober.rs`,
`auth/scan_tests.rs`, `auth/surfaces.rs`, `auth/surfaces_tests.rs`, `ctl/auth_tests.rs`.
No `cargo fmt` was run; only files with an actual code change were touched/staged.

## What changed

- Added `ProbeHintKind { OAuth (default), ApiToken }` in `cache.rs`, plus two new
  `#[serde(default)]` fields on `ServerEntry`: `hint_kind: ProbeHintKind` and
  `token_key: Option<String>` (backward-compatible with existing `status.json`).
- `prober.rs`: `impl From<ProbeKind> for ProbeHintKind` maps `ProbeKind::Http` →
  `ApiToken`, everything else (`GoogleRefresh`, `GatewayState`, `Stdio`) → `OAuth`
  (all three resolve to `Plan::Browser`/`Plan::Stdio` in `login::plan`, where
  `auth login` actually works). `AuthProber::execute` now populates both new
  fields on every `ServerEntry` it writes, reading `token_key` from the probe
  spec's existing `PARAM_TOKEN_KEY` param.
- `surfaces.rs::fix_action` gained two new parameters, `hint_kind` and
  `token_key`, and for `NeedsReauth` / `Expiring` / `Revoked` now branches on
  `hint_kind`: `ApiToken` produces a `"fix_config"` action whose label/command
  match `login::plan`'s actual Http-kind refusal text verbatim (`toolportctl
secret set {server} {key} (value on stdin or --value-env <VAR>), then
toolportctl auth probe --server {server} --force`, falling back to the
  literal `<KEY>` when no token key is known); `OAuth` keeps the previous
  `reauth`/`reconsent` + `toolportctl auth login {server}` behavior unchanged.
  `Misconfigured`/`Unreachable`/`Ok`/`Unknown` are untouched.

## Scoping decisions (binding, documented in code comments)

- **gcloud-kind**: out of scope. D-104's stitch-via-gcloud direction is not
  implemented anywhere in the probe/login code (no probe reads
  `STITCH_USE_SYSTEM_GCLOUD`), so `ProbeHintKind` deliberately has no third
  variant for it — there is nothing to route to yet. Documented inline in both
  `cache.rs` and `prober.rs`.
- **`ProbeKind::GatewayState`** (generic remote/vault-secret servers, e.g. the
  `srv-figma`/`srv-slack` fixtures in `ctl/auth_tests.rs`) is explicitly
  _not_ remapped to `ApiToken`: `login::plan`'s refusal wording for those is
  already different (`app_secret_step`, not the literal `secret set` CLI
  command the item's acceptance text asks for), and the item's spec text
  scopes the fix to "Http-kind servers (example: stitch)" only. Verified by
  reading `http_probes.rs`/`gateway_state.rs`/`login.rs` in full — no existing
  test needed changing for this reason.
- **Frontend**: no changes. `src/plus/api.ts`'s `AuthFixAction.action` union
  already includes `"fix_config"`, which the new ApiToken hint reuses.

## Tests — all three surfaces covered per probe kind

Ran from `/opt/toolport-plus/slots/s3/src-tauri`:

```
$ cargo test --no-default-features --lib auth::
cargo test: 226 passed, 3044 filtered out (1 suite, 7.28s)

$ cargo test --no-default-features --lib plus::ctl::
cargo test: 315 passed, 2955 filtered out (1 suite, 8.84s)
```

No failures, no ignored. Widened to `plus::ctl::` because `ctl/auth_tests.rs`
(the CLI-facing surface, `fix_cell`/`every_toolportctl_command_the_auth_surfaces_print_is_a_real_command`)
lives there, not under `auth::`.

New/updated tests, one per acceptance surface, in
`src-tauri/src/plus/auth/surfaces_tests.rs` unless noted:

- **Unit level** (`fix_descriptor_api_token_kind_points_at_secret_set_not_login`):
  for `NeedsReauth`/`Expiring`/`Revoked`, an `ApiToken`-kind "stitch" entry
  produces `action: "fix_config"` and the exact `secret set` command text
  (not "Sign in"); a missing `token_key` falls back to the literal `<KEY>`;
  an `OAuth`-kind entry for the same server/state still gets
  `toolportctl auth login stitch` — proves kind-sensitivity, not a global
  regression.
- **Probe table** (`rows_value_api_token_row_shows_secret_set_not_login`):
  exercises `rows_value()` (the actual JSON the CLI table and `plus.auth.rows`
  IPC render) with an `ApiToken`-kind row and asserts the full `fix` object,
  including `command`.
- **SessionStart hook** (`hook_context_uses_secret_set_label_for_api_token_kind`):
  asserts `hook()`'s `additionalContext` contains
  `"stitch: revoked (Set the API token for stitch)"` and does **not** contain
  `"Sign in"` / `"auth login"`.
- **Statusline** (`statusline_is_identical_regardless_of_hint_kind`): regression
  proof that `statusline()` output (counts/worst/text) is byte-for-byte
  identical for an `OAuth`-kind vs. `ApiToken`-kind entry in the same state —
  correct, since `statusline`/`statusline_from` only ever sees aggregate counts
  and the server name, never `fix.label`/`fix.command`.
- `fix_descriptor_per_state` updated to the new 4-arg `fix_action` signature
  (`ProbeHintKind::OAuth, None`), preserving all prior expected outputs.

Fixture-literal updates (no new tests needed, just the 2 new `ServerEntry`
fields / 4-arg `fix_action` calls to keep compiling):

- `auth/notify_tests.rs`: `status_of()` helper.
- `ctl/auth_tests.rs`: `every_state()` helper; tail loop of
  `every_toolportctl_command_the_auth_surfaces_print_is_a_real_command` updated
  to pass `ProbeHintKind::OAuth, None` for its synthetic "acme" server (not a
  real probe spec, so OAuth preserves prior/intended behavior). Confirmed via
  full read that `remote()`/`two_remotes()` fixtures in this file build
  `GatewayState`-kind servers, so their existing `"auth login srv-figma"` /
  `"signs in with a static token"` assertions are unaffected and needed no
  edits.
- `auth/scan_tests.rs`: one pre-existing assertion
  (`the_report_carries_counts_and_rows_for_the_probed_servers`) asserted
  `"toolportctl auth login beta"` for a synthetic `ProbeKind::Http` server —
  this was exactly the bug the item describes, so the expectation was updated
  to the correct `secret set` text (with the `<KEY>` fallback, since this
  test's registry sets no token-key param). This is a behavior fix, not a
  test-only churn: before this item, that server's hint really did
  dead-end.

## Not touched

- No `cargo fmt` run on the tree.
- No `toolport-migration` (this state repo) file touched or committed.
- Branch `mig/MIG-AUTH-9` not pushed.
- No secrets encountered; all tokens/keys in tests are synthetic
  (`STITCH_API_KEY` as a key _name_, `FAKE-*` placeholder values already
  present in the file).
