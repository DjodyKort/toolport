# Toolport+ fork

This is `DjodyKort/toolport`, the Toolport+ fork of `btsouth/toolport`. Claude Code reads this file; it does
not load `AGENTS.md` by itself, so the upstream guide is imported here:

@AGENTS.md

## Fork overrides

- AGENTS.md says "do not commit without approval". For the fork that is overridden (D-006): commit
  autonomously on `mig/<ID>` branches and merge only CI-green, fast-forward.
- Never run `cargo fmt` or `npm run format` on the tree (about 850 rewrites). Use `npm run format:check`.
- Commits: `[TAG] area: summary` (FIX IMP ADD REM REF MOV CLN PERF LINT), imperative, 72 chars max, body
  explains why. No AI attribution or Co-Authored-By lines.
- Push only `mig/*` branches (and `djody/main` by fast-forward). `main` is the upstream mirror.
- English everywhere; comments only for a non-obvious local reason.
- No real secrets: synthetic fixtures and a synthetic `TOOLPORT_SECRET_KEY` only.

## Layout

- `src-tauri/src/plus/` Rust extensions; one `pub mod plus;` in `lib.rs`, one generic IPC command
  `plus_invoke(command, args)` dispatching to handlers registered in `plus/mod.rs`.
- `src/plus/` React code and the `plusInvoke` wrapper (`src/plus/api.ts`). New IPC commands also need an entry in
  `src/test/browser-fixture.tsx`.
- Keep edits to upstream hot files (`desktop.rs`, `lib.rs`, `src/lib/api.ts`) minimal to ease merges.
- `src-tauri/tauri.fork.conf.json`: fork identifier and neutralised updater, merged at build time. Build with
  `npm run tauri:fork` (adds `--config src-tauri/tauri.fork.conf.json`); do not edit the base `identifier`.
- Egress: `brand::FORK_EGRESS_DISABLED` (Rust) and `src/lib/fork.ts` (TS) block share links, hosted Teams,
  update checks, the star prompt and btsouth/toolport.app links. Vitest sets `VITE_TOOLPORT_UPSTREAM_EGRESS=1`
  so upstream tests still cover those paths. The OAuth CIMD document stays.

## Verify

`CARGO_TARGET_DIR=... CARGO_BUILD_JOBS=4 nice -n 10 npm run verify`; scoped: `verify:frontend`, `verify:headless`.
Rust tests that touch the data dir use `registry::DataDirOverride`.
