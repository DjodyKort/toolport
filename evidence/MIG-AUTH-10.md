# MIG-AUTH-10 — Statusline and hook count only probed servers; prune the status cache

## Commit

`8d2b6451732289d6324bac7bcaae276dd0717c43` on `mig/MIG-AUTH-10` (off `djody/main@caa57e52`, local only, not pushed):

```
[FIX] auth: count only probed servers and prune the status cache
```

9 files changed, 294 insertions(+), 27 deletions(-). No `cargo fmt` run; no secrets (synthetic fixtures only).

## Root cause

Two related defects, both visible in the Mac session (research/2026-10-05-mac-session.md §4, D-104):

1. `surfaces::statusline` (surfaces.rs, was line ~227) and `surfaces::hook` (was ~247) built their counts from the
   unfiltered `rows(status, now)`, i.e. every entry in `status.json`. The probe table (`ctl/auth.rs::probe`,
   `scan::report_value`, `login::follow_up`) uses `rows_for(status, now, &run.servers())`, i.e. only the servers the
   current registry has a probe for.
2. Nothing ever removed an entry from `status.json` (`StatusFile.servers`/`profiles`): `AuthProber::execute` only
   inserts. A server removed from the registry (figma, 21:13:59) therefore kept its last `needs_reauth` entry forever,
   was no longer probed (so never refreshed), and still counted in every SessionStart hook / statusline (3 vs table 2).

## Fix

- `auth/surfaces.rs`: `statusline(status, now, registered)` and `hook(status, now, registered)` now count over
  `rows_for(status, now, registered)` (same row source as the probe table). Pure functions, still write-free.
- `auth/scan.rs`: new `probed_servers()` = every `spec.server` of `combined_registry(&read_registry()?)` (the set a
  probe run can show). `run()` now calls `prober.prune_unregistered()?` first, so every `auth probe`, `plus.auth.probe`
  and gateway due-scan tick prunes.
- `auth/cache.rs`: `StatusFile::prune(&ProbeRegistry) -> bool` drops `servers` entries with no probe in the registry
  and `profiles` (Google gate timestamps) no spec's `profile_gate_key()` owns; returns whether anything changed.
- `auth/prober.rs`: `AuthProber::prune_unregistered() -> Result<bool, String>`: no-op (no lock, nothing created) when
  `status.json` does not exist; otherwise lock, load, prune, save only if something was dropped.
- `ctl/auth.rs`: `render()` reads the status, takes `scan::probed_servers()` and passes it to statusline/hook. If the
  registry is unreadable it falls back to the cached keys (an unreadable registry must not blank the warnings).

## Tests

New:

- `auth::surfaces_tests::statusline_and_hook_count_exactly_the_registered_rows`: counts/worst/text/hook context equal
  `counts(rows_for(..))` of the registered subset.
- `auth::surfaces_tests::a_cached_failure_of_a_removed_server_is_neither_counted_nor_nagged`: the figma scenario.
- `auth::scan_tests::removing_a_server_drops_it_from_the_counts_and_from_the_cache`: real `AuthProber` + `scan::run`
  over a scratch dir: a `needs_reauth` slack and a whole Google profile are removed from the registry; the read
  surfaces drop it at once (cache still holds it, proving read surfaces do not write); after `scan::run` the entry is
  gone from `status.json` (`load_status`, `read_status` and the unfiltered `rows`), `profiles` lost `google:home`,
  surviving entries are byte-equal to before, and no probe ran.
- `auth::scan_tests::pruning_touches_the_cache_only_when_something_is_stale`: no cache -> nothing created; clean cache ->
  file bytes unchanged and `Ok(false)`; empty registry -> everything pruned once, then `Ok(false)`.
- `ctl::auth_tests::a_removed_server_leaves_the_statusline_hook_and_cache`: end to end through `toolportctl` with
  `write_registry`: probe two remotes (figma needs_reauth, slack ok) -> statusline 1; remove figma from the registry ->
  statusline 0 / `auth ok (1)`, hook has no `hookSpecificOutput`, `status.json` still has figma (read surfaces never
  write); `auth probe` -> `nothing due (1 registered)` and `status.json` no longer has figma, still has slack.

Changed: existing `statusline`/`hook` call sites in `surfaces_tests.rs` and `ctl/auth_tests.rs` pass the cached keys
as `registered` (helper `keys()`); `ctl/tests.rs::write_auth_status` keys renamed `alpha`/`beta` -> `srv-alpha`/`srv-beta`
(production keys are server ids; the old keys matched no registry server) and the two assertions that read them.

Mutation check: with the `prune_unregistered()` call in `scan::run` commented out, both
`removing_a_server_drops_it_from_the_counts_and_from_the_cache` and `a_removed_server_leaves_the_statusline_hook_and_cache`
FAIL (restored afterwards).

## Commands run (from `/opt/toolport-plus/slots/s3/src-tauri`, `CARGO_TARGET_DIR=/opt/toolport-plus/target/s3`, `nice -n 10 env CARGO_BUILD_JOBS=4`)

```
cargo test --lib plus::auth::        test result: ok. 161 passed; 0 failed
cargo test --lib plus::ctl::         test result: ok. 316 passed; 0 failed
cargo test --lib plus::attention::   test result: ok. 7 passed; 0 failed
cargo test --lib plus::status        test result: ok. 2 passed; 0 failed
cargo test --lib -- removing_a_server_drops pruning_touches statusline_and_hook_count_exactly a_cached_failure_of_a_removed a_removed_server_leaves
                                     5 new tests ok (plus one unrelated name match registry::tests::removing_a_server_drops_its_tool_scope)
cargo check                          clean apart from pre-existing warnings (none in touched files)
```

## Scope notes for the coordinator

- Only statusline and hook are filtered at read time (the item's named surfaces). `surfaces::status_summary` (`toolportctl
status` / `plus/status.rs`), `rows_value`/`rows_handler` (desktop Logins tab) and `attention/feed_auth.rs` still read the
  unfiltered `rows()`. They stop showing a removed server as soon as the next probe run prunes the cache (gateway due-scan
  every <= 60 s while a gateway runs, or any `auth probe`). Filtering them too would need fixture changes in
  `attention/tests.rs`, `ctl/tests.rs` (status golden) and is a possible follow-up item.
- "Registered" here = has a probe in `combined_registry` (stdio opt-in, Google, HTTP service, remote GatewayState), the same
  set the probe table can show; a registry server that lost its probe (hint removed, disabled-in-profile stdio) is pruned
  too, and its entry is rebuilt by the next probe if the probe returns.
- `auth_dir`/`status.json` format is unchanged (no STATUS_VERSION bump).
