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

The `context/cli-*` cases (MIG-CTX-9) record the text of the real `mcpm context <command>` (`rc=<exit code>` first, then
what a terminal shows at `COLUMNS=10000`, trailing spaces stripped); `tests/parity_context.rs` runs `toolportctl context`
in-process on the same inputs and compares. `profiles-reconcile` is replayed too, on the lines both engines produce:
launch profiles (D-022) are not mcpm's `CLAUDE_CONFIG_DIR` profile dirs, so the generated profile files, the shims text, the
`links*.json` observations and the profile action lines are skipped.

Other intentional deviations are applied the same way, by the generator's per-case `goldenlib/deviations.py` layer (not
by hand), and the case's `manifest.json` lists them under `deviations`. Currently:

- DEV-HRD11-1 (D-049): `skills/zed-append-existing` `repo/.rules` ends the text after the managed block with one newline
  instead of mcpm's extra one.
- DEV-GFX4-1 (D-070, MIG-GFX-4): in `skills/claude-code-multiline-description` a multi-line `description:` is an indented
  block scalar. mcpm splices the raw text between double quotes, which leaves continuation lines in column 0 (and unescaped
  quotes) and makes Claude Code hide the skill from the model.
- DEV-GFX4-2 (D-070, MIG-GFX-4): in `skills/claude-code-project` and `skills/claude-code-global` the skill's
  `paths: **/*.py` is written as `paths: "**/*.py"`; the unquoted form is a YAML alias and fails a strict parse.
