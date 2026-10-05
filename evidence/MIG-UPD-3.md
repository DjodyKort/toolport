# MIG-UPD-3 evidence

Branch `mig/MIG-UPD-3` off `origin/djody/main@d0fab43c`.

```
0e5c8771 [FIX] catalog-view: stop swallowing registry search errors
30c59e6c [FIX] ctl: surface registry errors in server search, not a hard fail
996d707a [FIX] catalog: split registry timeout, cache, keep curated hits
```

## Files changed

- `src-tauri/src/catalog.rs` — split connect/read timeouts, `RegistryError`
  classification, `CatalogSearch { entries, registry_error }` return type,
  10 min in-memory per-query cache with stale-fallback, gatewaylog line,
  6 new tests + mock-TCP test helpers.
- `src-tauri/src/desktop.rs` — `search_catalog` IPC command returns
  `catalog::CatalogSearch` instead of `Result<Vec<CatalogEntry>, String>`.
- `src-tauri/src/linux_native/catalog.rs` (gtk-desktop feature) — both call
  sites updated to `catalog::search(&query).entries`.
- `src-tauri/src/plus/ctl/server.rs` — `run_search` returns
  `(Vec<CatalogEntry>, Option<RegistryError>)`; `search()` appends a
  plain-language note and a `registryError` key to the `--json` envelope
  only when the registry call failed (success path unchanged).
- `src/lib/api.ts`, `src/lib/types.ts` — `CatalogSearchResult`/`RegistryError`
  types, `searchCatalog` returns the new shape.
- `src/plus/types/server.ts` — `registryError: opt(obj({kind, message}))`
  added to `serverSearchData` shape.
- `src/components/CatalogView.tsx` — stops swallowing the error in `.catch`;
  non-blocking warning banner when curated hits exist alongside a
  `registryError`; old blocking "couldn't reach" message kept only for
  `kind === "connectionFailed"`.
- `src/components/CatalogView.stacks.test.tsx` — updated mock to the new
  `{ entries: [] }` shape.
- `src/components/CatalogView.search.test.tsx` (new) — 5 tests covering the
  banner/blocking-message split across timeout/connectionFailed × with/
  without curated hits.

## Commands run and results

Rust, targeted (final, on committed state):

```
cargo test --no-default-features --lib catalog::
test result: ok. 35 passed; 0 failed; 0 ignored; 0 measured; 3231 filtered out
```

Rust, `ctl_contract` (only unrelated pre-existing failure):

```
cargo test --no-default-features --test ctl_contract
contract_coverage ... ok
no_output_carries_a_canary_secret ... ok (flaked once on a busy fixed
  port 29214 under 3 concurrent heavy jobs across slots; passes alone)
every_case_matches_its_golden_envelope ... FAILS on `attention-ls.default`
  pre-existing on clean HEAD (verified via `git stash` + rerun before any
  of my edits) — unrelated "task resume" action field, not server-search.
  server-search's own golden is byte-identical (no rebless needed).
```

Rust, full headless suite (`cargo test --no-default-features --lib --bins
--tests --no-fail-fast`, logged to /tmp/full_test_run.log):

```
error: 2 targets failed: `ctl_contract`, `selfmcp_envelopes`
```

Both failures are the same two pre-existing/environmental issues:

1. `attention-ls.default` / `attention_ls.default` golden drift (also seen
   in `selfmcp_envelopes::every_tool_call_matches_its_golden_result`) —
   confirmed pre-existing on unmodified `djody/main` HEAD via `git stash`.
2. `no_output_carries_a_canary_secret` hit `port 29214 is needed for the
fake proxy: Address already in use` — port contention from running
   three heavy cargo jobs across s1/s2/s3 at once; passes in isolation and
   is unrelated to catalog/registry code.
   Every other target in the full run passed (3263 passed in the lib suite,
   0 failed elsewhere).

TypeScript/vitest, targeted (final, on committed state):

```
npx vitest run src/components/CatalogView.search.test.tsx \
  src/components/CatalogView.stacks.test.tsx src/plus/bridge/data.test.ts
Test Files  3 passed (3)
Tests  15 passed (15)
```

Formatting: lint-staged (husky pre-commit, not run by hand) applied
prettier + eslint --fix to the 6 touched TS/TSX files on commit; no
manual `cargo fmt` was run anywhere.

## Acceptance checklist (from prompt)

1. Split timeouts (5s connect / 60s read) — done, `AgentBuilder::timeout_connect`
   / `timeout_read`, covered by `search_registry_timeout_keeps_curated_hits_and_reports_timeout`.
2. `search()` never drops curated hits on a registry error — done,
   `CatalogSearch { entries, registry_error }`, no more bare `?`.
3. ~10 min in-memory per-query cache with stale-fallback — done,
   `registry_cache_serves_a_repeat_query_without_a_new_network_call`,
   `registry_cache_expires_and_refetches_after_ttl`,
   `registry_falls_back_to_last_good_answer_when_a_stale_refetch_fails`.
4. Real error logged to gateway.log — done, `crate::gatewaylog::append`
   in `registry_search_cached_from`'s error branch.
5. `CatalogView.tsx` stops swallowing the error, shows a banner for a
   live-but-slow/erroring registry, keeps the old message only for a
   genuine connection failure — done, see `CatalogView.search.test.tsx`.
6. `toolportctl server search` carries the same behaviour, existing
   `--json` golden unaffected (field only added when `Some`) — done;
   verified no rebless needed (the only ctl_contract golden drift is the
   pre-existing unrelated `attention-ls.default` case).

Distinct error-kind strings for timeout vs connection-refused — done,
`registry_error_kinds_are_distinct_strings` plus the two classifier tests.
`src/plus/gui-parity.json` — not touched, no new row needed (confirmed:
existing `server search` row stands, this is a behaviour fix).

## Deviations / follow-ups

- **Upstream bug to flag**: the research doc (`research/2026-10-05-mac-session.md`
  §1) notes the same bug exists in `btsouth/toolport` (one flat 20s timeout,
  bare `?` propagation, swallowed error in the GUI). Per WORKER-RULES I did
  not open anything upstream; coordinator should draft something under
  `research/upstream-drafts/`.
- **Not locally verifiable**: `src-tauri/src/linux_native/catalog.rs` is
  gated behind the `gtk-desktop` feature (Linux-native GTK preview). I have
  no `gtk4`/`libadwaita-1` dev packages and no sudo to install them
  (`pkg-config --exists gtk4` / `libadwaita-1` both report missing), so
  `cargo check --no-default-features --features gtk-desktop --bin toolport-gtk`
  could not be run locally. The two call sites were changed minimally
  (`crate::catalog::search(&query)?` → `crate::catalog::search(&query).entries`,
  with a one-line WHY comment referencing D-101) by code review; CI's
  "Linux-native GTK preview" job will be the first real compile check.
- **Pre-existing anomaly observed, out of scope**: `attention-ls.default` /
  `attention_ls.default` golden drift in `ctl_contract` and
  `selfmcp_envelopes` (an unrelated "task resume" action field). Confirmed
  pre-existing on clean `djody/main@d0fab43c` via `git stash` before/after
  comparison; not touched.
- **Pre-existing flake observed, out of scope**: `no_output_carries_a_canary_secret`
  needs a fixed port (29214) for a fake proxy and can lose a race against
  other concurrent heavy cargo jobs on the same host; passes in isolation.

## Honest status

Everything in the 6-point required fix and the acceptance list is done and
covered by behaviour tests, all green on the committed state. The one gap
is the `gtk-desktop` feature build, which cannot be compiled on this
machine (no root to install GTK4/libadwaita dev headers) — flagged above
for CI to catch.
