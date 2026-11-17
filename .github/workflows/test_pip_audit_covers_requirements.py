#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Every tracked requirements.txt (outside vendored third-party trees) is audited by .github/workflows/pip-audit.yml.

daemons/bench_mcp/glasgow/requirements.txt (104 hash-pinned lines) was never scanned by the workflow. Dependabot is not
asked to bump it: pin_requirements.py generates it with hashes and says not to edit it by hand."""
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class TestPipAuditCoversRequirements(unittest.TestCase):
    def test_every_requirements_file_is_audited(self):
        workflow = (ROOT / ".github" / "workflows" / "pip-audit.yml").read_text()
        tracked = subprocess.run(["git", "ls-files", "requirements*.txt", "**/requirements*.txt"], cwd=ROOT,
                                 capture_output=True, text=True, check=True).stdout.split()
        self.assertTrue(tracked)
        missing = [p for p in tracked if "/vendor/" not in p and f"- {p}\n" not in workflow]
        self.assertEqual(missing, [], "add it to the matrix and trigger paths in pip-audit.yml")


if __name__ == "__main__":
    unittest.main()
