#!/usr/bin/env python3
"""Fail on any failing test that is not on the macOS allowlist.

Usage: check-tests.py <test output file> <exit code> [allowlist file]

Reads `cargo test --no-fail-fast` or `cargo nextest run` output; either way every
target executes. Tolerates only the tests that fail the same way on pristine
upstream on this Mac.
"""
import re
import sys
from pathlib import Path

DEFAULT_ALLOWLIST = Path(__file__).with_name("test-macos-allowlist.txt")
FAILED = re.compile(r"^test (\S+) \.\.\. FAILED$")
# nextest: "        FAIL [   2.101s] (2483/3971) conduit secret_refs::tests::x" (counter optional, binary id, test name)
NEXTEST_FAILED = re.compile(
    r"^\s+(?:FAIL|TIMEOUT|SIGABRT|SIGSEGV|SIGBUS|SIGILL|SIGKILL|EXITSIG\S*|LEAK-FAIL) \[\s*[\d.]+s\] (?:\(\d+/\d+\) )?\S+ (\S+)$"
)


def failed_tests(text):
    return sorted(
        {
            m.group(1)
            for line in text.splitlines()
            if (m := FAILED.match(line) or NEXTEST_FAILED.match(line))
        }
    )


def read_allowlist(path):
    return {
        line.split("#")[0].strip()
        for line in Path(path).read_text().splitlines()
        if line.split("#")[0].strip()
    }


def main(argv):
    output = Path(argv[1]).read_text()
    exit_code = int(argv[2])
    allowed = read_allowlist(argv[3] if len(argv) > 3 else DEFAULT_ALLOWLIST)

    failed = failed_tests(output)
    unexpected = [name for name in failed if name not in allowed]
    for name in unexpected:
        print(f"unexpected test failure: {name}")
    if exit_code != 0 and not failed:
        print("the test run failed without a failing test (build error or crashed test binary)")
        return 1
    print(f"{len(failed) - len(unexpected)} known macOS-only failure(s) ignored, {len(unexpected)} unexpected")
    return 1 if unexpected else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
