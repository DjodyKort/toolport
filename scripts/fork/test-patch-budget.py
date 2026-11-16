#!/usr/bin/env python3
"""Self-test for patch-budget.py against a throwaway git repository."""
import contextlib
import importlib.util
import io
import subprocess
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "patch_budget", Path(__file__).with_name("patch-budget.py")
)
patch_budget = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(patch_budget)


def git(root, *args):
    subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@t", *args],
        cwd=root,
        check=True,
        capture_output=True,
    )


class PatchBudgetTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        git(self.root, "init", "-q", "-b", "main")
        for name in ("a.rs", "b.rs", "c.rs"):
            (self.root / name).write_text("1\n2\n3\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-q", "-m", "upstream")
        git(self.root, "tag", "upstream")
        self.budget = self.root / "budget.txt"

    def tearDown(self):
        self.tmp.cleanup()

    def patched(self):
        return patch_budget.patched_upstream_files("upstream", self.root)

    def run_main(self, *extra):
        with contextlib.redirect_stdout(io.StringIO()):
            return patch_budget.main(
                ["--root", str(self.root), "--base", "upstream", "--budget", str(self.budget), *extra]
            )

    def test_fork_owned_and_added_files_are_not_patches(self):
        (self.root / "plus").mkdir()
        (self.root / "plus/x.rs").write_text("x\n")
        (self.root / "new_fork_file.rs").write_text("y\n")
        (self.root / "a.rs").write_text("1\n2\nchanged\n")
        git(self.root, "add", ".")
        self.assertEqual(self.patched(), {"a.rs": 2})

    def test_deleting_an_upstream_file_counts(self):
        git(self.root, "rm", "-q", "b.rs")
        self.assertEqual(self.patched(), {"b.rs": 3})

    def test_new_file_and_growth_fail_and_shrinking_passes(self):
        (self.root / "a.rs").write_text("1\n2\nchanged\n")
        git(self.root, "add", ".")
        self.assertEqual(self.run_main("--update"), 0)
        self.assertEqual(self.run_main(), 0)

        (self.root / "a.rs").write_text("1\nx\ny\n")
        self.assertEqual(self.run_main(), 1)

        (self.root / "a.rs").write_text("1\n2\n3\n")
        (self.root / "c.rs").write_text("1\n2\nchanged\n")
        self.assertEqual(self.run_main(), 1)

        (self.root / "c.rs").write_text("1\n2\n3\n")
        self.assertEqual(self.run_main(), 0)


if __name__ == "__main__":
    unittest.main()
