#!/usr/bin/env python3
"""Self-test for check-tests.py."""
import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_tests", Path(__file__).with_name("check-tests.py")
)
check_tests = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_tests)


class CheckTestsTest(unittest.TestCase):
    def run_check(self, output, exit_code=101):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "log"
            log.write_text(output)
            allowlist = Path(tmp) / "allow"
            allowlist.write_text("# c\nmod::known  # why\n")
            with contextlib.redirect_stdout(io.StringIO()):
                return check_tests.main(["x", str(log), str(exit_code), str(allowlist)])

    def test_known_failure_passes(self):
        self.assertEqual(self.run_check("test mod::known ... FAILED\ntest mod::ok ... ok\n"), 0)

    def test_other_failure_fails(self):
        self.assertEqual(self.run_check("test mod::known ... FAILED\ntest mod::new ... FAILED\n"), 1)

    def test_failure_without_a_failing_test_fails(self):
        self.assertEqual(self.run_check("error: could not compile\n"), 1)

    def test_nextest_known_failure_passes(self):
        out = "        FAIL [   2.101s] (2483/3971) conduit mod::known\n        PASS [   0.1s] conduit mod::ok\n"
        self.assertEqual(self.run_check(out), 0)

    def test_nextest_timeout_and_other_failures_fail(self):
        out = "     TIMEOUT [  60.0s] conduit::bin mod::new\n"
        self.assertEqual(self.run_check(out), 1)

    def test_clean_run_passes(self):
        self.assertEqual(self.run_check("test mod::ok ... ok\n", exit_code=0), 0)


if __name__ == "__main__":
    unittest.main()
