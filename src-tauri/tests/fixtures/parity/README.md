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

Command text deviates from mcpm on purpose (D-042): the generator's names layer maps the commands mcpm tells the
user to run onto their `toolportctl` equivalents, so `context/` goldens say `toolportctl context sync`, not `mcpm context sync`.
Paths, file names and identifiers such as `.config/mcpm` stay as mcpm writes them.

Other intentional deviations are applied the same way, by the generator's per-case `goldenlib/deviations.py` layer (not
by hand), and the case's `manifest.json` lists them under `deviations`. Currently one: DEV-HRD11-1 (D-049), where
`skills/zed-append-existing` `repo/.rules` ends the text after the managed block with one newline instead of mcpm's extra one.
