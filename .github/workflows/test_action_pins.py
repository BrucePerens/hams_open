#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is proprietary and confidential.
"""Every third-party GitHub Action in the workflows is pinned to a full commit SHA.

A tag such as actions/checkout@v7 can be moved by whoever controls that repository, and the
build-relay workflow holds the release signing and publish secrets. Dependabot (github-actions
ecosystem, .github/dependabot.yml) updates SHA pins and keeps the `# vN` comment current."""
import re
import unittest
from pathlib import Path

WORKFLOW_DIR = Path(__file__).resolve().parent
USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(\S+)")
PINNED = re.compile(r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")


class TestActionPins(unittest.TestCase):
    def test_every_action_is_pinned_to_a_commit_sha(self):
        unpinned = []
        for path in sorted(WORKFLOW_DIR.glob("*.yml")):
            for number, line in enumerate(path.read_text().splitlines(), 1):
                found = USES.match(line)
                if found and not found.group(1).startswith("./") and not PINNED.match(found.group(1)):
                    unpinned.append(f"{path.name}:{number}: {found.group(1)}")
        self.assertEqual(unpinned, [], "actions must be pinned to a 40-hex commit SHA")


if __name__ == "__main__":
    unittest.main()
