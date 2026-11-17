#!/usr/bin/env python3
"""Self-test for env.sh: it resolves the repo root when sourced from bash or zsh."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ENV_SH = ROOT / "scripts" / "fork" / "env.sh"


def sourced_root(shell, cwd, path):
    result = subprocess.run(
        [shell, "-c", f'source "{path}" && printf %s "$FORK_ROOT"'],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode, result.stdout, result.stderr


class EnvShTest(unittest.TestCase):
    def check(self, shell):
        with tempfile.TemporaryDirectory() as elsewhere:
            absolute = sourced_root(shell, elsewhere, ENV_SH)
            relative = sourced_root(shell, ENV_SH.parent, "./env.sh")
        for code, out, err in (absolute, relative):
            self.assertEqual((code, out), (0, str(ROOT)), err)

    def test_bash(self):
        self.check("bash")

    def test_system_bash(self):
        if not Path("/bin/bash").exists():
            self.skipTest("no /bin/bash")
        self.check("/bin/bash")

    def test_zsh(self):
        if shutil.which("zsh") is None:
            self.skipTest("zsh is not installed")
        self.check("zsh")


if __name__ == "__main__":
    unittest.main()
