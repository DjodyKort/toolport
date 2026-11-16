#!/usr/bin/env python3
"""Keep the fork's edits to upstream files inside a committed budget.

Lists the files that exist in the upstream ref and differ in the working tree,
leaving out fork-owned paths and files the fork added. Compares them with
scripts/fork/patch-budget.txt (`<changed lines><TAB><path>`, changed = added +
removed). Fails when a new upstream file is patched or a patch grows past its
budget. A smaller patch passes with a hint to tighten the budget.

Usage:
  patch-budget.py [--base REF] [--budget FILE]            check
  patch-budget.py [--base REF] [--budget FILE] --update   rewrite the budget
"""
import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FORK_OWNED_PREFIXES = ("plus/", "scripts/fork/")
DEFAULT_BUDGET = ROOT / "scripts/fork/patch-budget.txt"


def git(*args, cwd):
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout


def patched_upstream_files(base, cwd):
    """Map path -> changed lines for upstream files the working tree modifies or deletes."""
    status = {}
    fields = git("diff", "--name-status", "--no-renames", "-z", base, cwd=cwd).split("\0")
    for code, path in zip(fields[0::2], fields[1::2]):
        status[path] = code
    patched = {}
    fields = git("diff", "--numstat", "--no-renames", "-z", base, cwd=cwd).split("\0")
    for entry in filter(None, fields):
        added, removed, path = entry.split("\t", 2)
        if status.get(path) == "A" or path.startswith(FORK_OWNED_PREFIXES):
            continue
        patched[path] = 1 if added == "-" else int(added) + int(removed)
    return patched


def read_budget(path):
    budget = {}
    for number, line in enumerate(Path(path).read_text().splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        lines, _, file = line.partition("\t")
        if not lines.isdigit() or not file:
            raise SystemExit(f"{path}:{number}: expected '<changed lines><TAB><path>'")
        budget[file] = int(lines)
    return budget


def write_budget(path, patched, base):
    header = (
        f"# Upstream files the fork patches, against {base}.\n"
        "# <changed lines (added + removed)><TAB><path>. Regenerate with\n"
        "# scripts/fork/patch-budget.py --update and review the diff like code.\n"
    )
    body = "".join(f"{lines}\t{file}\n" for file, lines in sorted(patched.items()))
    Path(path).write_text(header + body)


def compare(patched, budget):
    failures, hints = [], []
    for file, lines in sorted(patched.items()):
        if file not in budget:
            failures.append(f"new upstream file patched: {file} ({lines} lines)")
        elif lines > budget[file]:
            failures.append(f"patch grew: {file} has {lines} changed lines, budget {budget[file]}")
        elif lines < budget[file]:
            hints.append(f"{file} shrank from {budget[file]} to {lines}")
    for file in sorted(set(budget) - set(patched)):
        hints.append(f"{file} is no longer patched")
    return failures, hints


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--base", help="upstream ref (default: scripts/fork/upstream-ref)")
    parser.add_argument("--budget", default=str(DEFAULT_BUDGET))
    parser.add_argument("--update", action="store_true", help="rewrite the budget file")
    parser.add_argument("--root", default=str(ROOT), help=argparse.SUPPRESS)
    args = parser.parse_args(argv)

    root = Path(args.root)
    base = args.base or (root / "scripts/fork/upstream-ref").read_text().split()[0]
    patched = patched_upstream_files(base, root)

    if args.update:
        write_budget(args.budget, patched, base)
        print(f"wrote {len(patched)} files to {args.budget}")
        return 0

    failures, hints = compare(patched, read_budget(args.budget))
    total = sum(patched.values())
    print(f"{len(patched)} upstream files patched against {base}, {total} changed lines")
    for hint in hints:
        print(f"  can tighten: {hint}")
    for failure in failures:
        print(f"  FAIL {failure}")
    if failures:
        print("Move the logic into plus/, or run patch-budget.py --update and justify it in review.")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
