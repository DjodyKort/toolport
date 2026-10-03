# Parity goldens (class 1, synthetic)

Normalized output of the mcpm fork (SHA in `PARITY.lock`'s first line) over synthetic inputs, one
directory per case: `<area>/<case>/{tree/,manifest.json}`. `PARITY.lock` holds a sha256 per file.

- `tests/parity_goldens_lock.rs` verifies every file against `PARITY.lock`.
- `tests/common/parity.rs` is the std-only comparison harness used by the per-area parity tests
  (class A byte-for-byte after normalizers, class B canonical JSON, class C contract tests).
- Class-2 fixtures live in the private docs repo; tests skip them unless `PARITY_PRIVATE_DIR` is set
  (`scripts/parity-run <dir>`).

Never edit these files by hand and never bless in CI (`PARITY_BLESS` is rejected when `CI` is set).
Goldens change only through a `golden-bump` migration item: regenerate with
`scripts/golden/gen-golden --all --out <dir>` in the migration repo, replace this directory
(without `run-meta.json`), and record the new mcpm SHA and the reason in the item.
