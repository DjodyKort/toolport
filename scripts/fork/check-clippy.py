#!/usr/bin/env python3
"""Fail on any clippy error that is not on the macOS allowlist.

Usage: check-clippy.py <clippy output file> <cargo exit code> [allowlist file]

`cargo clippy --keep-going` exits non-zero for the pre-existing macOS-only
errors; this keeps every other error fatal, including a new error in a file
that also has an allowed one.
"""
import re
import sys
from pathlib import Path

DEFAULT_ALLOWLIST = Path(__file__).with_name("clippy-macos-allowlist.txt")
ERROR = re.compile(r"^error(?:\[\w+\])?: (.*)$")
LOCATION = re.compile(r"^\s*--> (.*?):\d+:\d+$")
SUMMARY = ("could not compile", "aborting due to")


def parse_errors(text):
    """Yield (path, message) for each diagnostic error; (None, message) without a location."""
    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = ERROR.match(line)
        if not match or match.group(1).startswith(SUMMARY):
            continue
        path = None
        for following in lines[index + 1 : index + 4]:
            location = LOCATION.match(following)
            if location:
                path = location.group(1)
                break
        yield path, match.group(1)


def read_allowlist(path):
    allowed = set()
    for line in Path(path).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            file, _, message = line.partition("\t")
            allowed.add((file, message))
    return allowed


def main(argv):
    output = Path(argv[1]).read_text()
    exit_code = int(argv[2])
    allowlist = Path(argv[3]) if len(argv) > 3 else DEFAULT_ALLOWLIST
    allowed = read_allowlist(allowlist)

    errors = sorted(set(parse_errors(output)), key=str)
    unexpected = [error for error in errors if error not in allowed]
    for path, message in unexpected:
        print(f"unexpected clippy error: {path or '(no location)'}: {message}")
    if exit_code != 0 and not errors:
        print("cargo clippy failed without a diagnostic error (see the log)")
        return 1
    known = len(errors) - len(unexpected)
    print(f"{known} known macOS-only error(s) ignored, {len(unexpected)} unexpected")
    return 1 if unexpected else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
