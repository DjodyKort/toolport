# D-144/D-145 Setup & backup

Branch djody/v2-setup-export @ 2a424fce2454ba962d005cb487ea76e165b7b99a

- local-ci (fork-scripts, patch-budget, rustfmt, plus-fmt, seam-tests, fork-ts): ok
- plus-clippy: my crates clean after the fix; the api crate fails on an unrelated "items after a test module" in api/src/memory.rs
- plus-test: nextest is not installed here; ran cargo test -p toolport-plus setup:: -> 28 passed; plus_gui_parity 16 passed; selfmcp_coverage ok
- ctl contract goldens: new setup-* goldens added; status/doctor/mcp-call/export-preview/library-push drift here only through this host (gateway path, gitleaks present, byte sizes), their goldens are untouched
- gitleaks: 1 finding, plus/core/src/servers/env.rs:456 in e7bdad29, already on origin/djody/v2
- vitest src/plus 409 passed; tsc, eslint --max-warnings 0, prettier --check clean
- npm run build fails: @fontsource-variable/inter is missing from this slot's node_modules, so bench:bundle was not run
- Not done: the Sync view of the page; sealed-secrets restore test
