#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is proprietary and confidential.
"""Every daemon crate with a Cargo.lock has a cargo entry in .github/dependabot.yml.

bench_mcp sat on an rmcp release with RUSTSEC-2026-0462 because Dependabot did not watch it."""
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class TestDependabotCoversCargo(unittest.TestCase):
    def test_every_daemon_lockfile_directory_is_watched(self):
        config = (ROOT / ".github" / "dependabot.yml").read_text()
        tracked = subprocess.run(["git", "ls-files", "daemons/*/Cargo.lock"], cwd=ROOT,
                                 capture_output=True, text=True, check=True).stdout.split()
        tracked = [p for p in tracked if p.count("/") == 2]  # not fuzz/
        missing = [p for p in tracked if f'directory: "/{Path(p).parent}"' not in config]
        self.assertEqual(missing, [], "add a cargo entry to .github/dependabot.yml")


if __name__ == "__main__":
    unittest.main()
