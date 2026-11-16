#!/usr/bin/env python3
"""Self-test for check-clippy.py."""
import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_clippy", Path(__file__).with_name("check-clippy.py")
)
check_clippy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_clippy)

ALLOWED = "error: function `running` is never used\n   --> tests/daemon_crash_recovery.rs:123:4\n"
SUMMARY = "error: could not compile `conduit` (test \"x\") due to 1 previous error\n"


class CheckClippyTest(unittest.TestCase):
    def run_check(self, output, exit_code=101):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "log"
            log.write_text(output)
            allowlist = Path(tmp) / "allow"
            allowlist.write_text(
                "# comment\ntests/daemon_crash_recovery.rs\tfunction `running` is never used\n"
            )
            with contextlib.redirect_stdout(io.StringIO()):
                return check_clippy.main(["x", str(log), str(exit_code), str(allowlist)])

    def test_allowed_error_passes(self):
        self.assertEqual(self.run_check(ALLOWED + SUMMARY), 0)

    def test_other_error_in_the_same_file_fails(self):
        other = "error: unused variable: `y`\n --> tests/daemon_crash_recovery.rs:9:5\n"
        self.assertEqual(self.run_check(ALLOWED + other + SUMMARY), 1)

    def test_failure_without_a_diagnostic_fails(self):
        self.assertEqual(self.run_check("error: failed to run custom build command\n"), 1)
        self.assertEqual(self.run_check("nothing useful\n"), 1)

    def test_clean_run_passes(self):
        self.assertEqual(self.run_check("Finished\n", exit_code=0), 0)


if __name__ == "__main__":
    unittest.main()
