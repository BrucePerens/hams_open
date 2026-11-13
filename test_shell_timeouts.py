#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Every curl, wget, ssh, scp and rsync command in a tracked shell script names a time limit (timeout audit 2026-11-12:
a hung server or host must fail the script, not hang it)."""
import os
import re
import subprocess
import unittest

ROOT = os.path.dirname(os.path.abspath(__file__))
COMMAND = re.compile(r"(?:^\s*|[|;&(]\s*|\$\(\s*|\b(?:retry|then|do|else)\s+)(curl|wget|ssh|scp|rsync)\s+(?![+=,])")
LIMIT = re.compile(r"--max-time|\s-m\s*\d|--connect-timeout|ConnectTimeout|\s-T\s*\d|SSH_TIMEOUTS|--timeout")
SKIP = re.compile(r"^\s*(echo|printf|log|say|ok|no|die|warn|command|case|\[)\b|command -v|which ")
EXEMPT = set()


def tracked_scripts():
    out = subprocess.run(["git", "-C", ROOT, "ls-files", "*.sh"], capture_output=True, text=True, check=True).stdout
    return [p for p in out.splitlines() if os.path.isfile(os.path.join(ROOT, p)) and "vendor/" not in p and p not in EXEMPT]


def logical_lines(path):
    """(first line number, text) with backslash continuations joined and comment lines dropped."""
    joined, start, buffer = [], 0, ""
    with open(os.path.join(ROOT, path), encoding="utf-8", errors="replace") as handle:
        for number, line in enumerate(handle, 1):
            if not buffer:
                start = number
            if line.rstrip("\n").endswith("\\"):
                buffer += line.rstrip("\n")[:-1] + " "
                continue
            buffer += line.rstrip("\n")
            if not buffer.lstrip().startswith("#"):
                joined.append((start, buffer))
            buffer = ""
    return joined


class ShellTimeoutTests(unittest.TestCase):
    def test_every_network_command_in_a_script_has_a_time_limit(self):
        missing = []
        for path in tracked_scripts():
            for number, text in logical_lines(path):
                if SKIP.search(text) or not COMMAND.search(text) or LIMIT.search(text):
                    continue
                missing.append("%s:%d: %s" % (path, number, text.strip()[:100]))
        self.assertEqual(missing, [])


if __name__ == "__main__":
    unittest.main()
