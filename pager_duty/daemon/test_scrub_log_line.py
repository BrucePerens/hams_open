# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A tailed log line is scrubbed before it reaches the incident or the webhook. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_scrub_log_line.py
"""
# Tests [@ANCHOR: pager_duty:scrub_log_line]
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class ScrubLogLine(unittest.TestCase):
    def test_removes_sensitive_parts_and_keeps_the_rest(self):
        line = (
            "ERROR login failed for k6bp@example.com password=hunter2 token: abc123 "
            "Authorization: Bearer xyz.789 GET https://x.example/a?key=1 disk full"
        )
        out = gm.scrub_log_line(line)
        for secret in ("k6bp@example.com", "hunter2", "abc123", "xyz.789", "x.example"):
            self.assertNotIn(secret, out)
        self.assertIn("login failed", out)
        self.assertIn("disk full", out)

    def test_quoted_value_with_spaces_is_removed_whole(self):
        out = gm.scrub_log_line('db_password="two words" retry')
        self.assertNotIn("words", out)
        self.assertIn("retry", out)

    def test_plain_line_unchanged(self):
        self.assertEqual(gm.scrub_log_line("Failed to renew certificate"), "Failed to renew certificate")


if __name__ == "__main__":
    unittest.main()
