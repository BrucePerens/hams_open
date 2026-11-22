#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""No tracked file or directory name holds a character Windows forbids (: * ? " < > | or a control character), ends in a
dot or space, or is a reserved device name, so the repository checks out on NTFS. Recorded sysfs fixtures store ':' as '%3A'
(bench_mcp usb_hub testkit decodes it)."""
import os
import re
import subprocess
import unittest

ROOT = os.path.dirname(os.path.abspath(__file__))
BAD_CHARS = re.compile(r'[:*?"<>|\x00-\x1f]')
RESERVED = re.compile(r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$", re.I)


def bad_part(part):
    """Why a single path component is not Windows-safe, or None."""
    if BAD_CHARS.search(part):
        return "illegal character"
    if part.endswith((".", " ")):
        return "trailing dot or space"
    if RESERVED.match(part):
        return "reserved device name"
    return None


def problems(paths):
    found = []
    for path in paths:
        for part in path.split("/"):
            why = bad_part(part)
            if why:
                found.append(f"{path}: {why}")
                break
    return found


class WindowsSafeFilenames(unittest.TestCase):
    def test_detector(self):
        self.assertEqual(len(problems(["a/1-0:1.0", "b/x*", "c/d.", "e/nul.txt", "f/ok%3A1", "g/h i"])), 4)

    def test_tracked_names(self):
        out = subprocess.run(["git", "-C", ROOT, "ls-files", "-z"], capture_output=True, text=True, check=True).stdout
        bad = problems(p for p in out.split("\0") if p)
        self.assertEqual(bad[:20], [], f"{len(bad)} tracked names are not Windows-safe")


if __name__ == "__main__":
    unittest.main()
