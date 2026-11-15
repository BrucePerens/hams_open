# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A tailed log line is scrubbed before it reaches the incident or the webhook. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_scrub_log_line.py
"""
# Tests [@ANCHOR: pager_duty:scrub_log_line]
import os
import sys
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


def assert_scales_linearly(test, run, small, factor=8):
    """Fails when `run(n)` costs more than 3x `factor` times as much for `factor` times the input (linear is
    `factor`x, quadratic `factor`^2 x). Compares this machine's own timings, so load does not fail it; best of
    three on each side drops a one-off stall."""
    def cost(n):
        costs = []
        for _ in range(3):
            started = time.perf_counter()
            run(n)
            costs.append(time.perf_counter() - started)
        return min(costs)
    small_cost = cost(small)
    test.assertLess(cost(small * factor), max(small_cost, 0.02) * factor * 3)


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

    def test_a_very_long_line_is_scrubbed_in_linear_time(self):
        assert_scales_linearly(self, lambda n: gm.scrub_log_line("a" * n + " k6bp@example.com"), 20000)
        out = gm.scrub_log_line("a" * 200000 + " k6bp@example.com")
        self.assertNotIn("k6bp@example.com", out)

    def test_quoted_value_with_spaces_is_removed_whole(self):
        out = gm.scrub_log_line('db_password="two words" retry')
        self.assertNotIn("words", out)
        self.assertIn("retry", out)

    def test_a_basic_authorization_header_is_removed_whole(self):
        out = gm.scrub_log_line("sent Authorization: Basic dXNlcjpwYXNzd29yZA== to upstream")
        self.assertNotIn("dXNlcjpwYXNzd29yZA", out)
        self.assertIn("to upstream", out)

    def test_a_cookie_header_with_several_pairs_is_removed_whole(self):
        out = gm.scrub_log_line("got Cookie: sid=abc123; csrf=xyz789; theme=dark then retry")
        for secret in ("abc123", "xyz789", "theme"):
            self.assertNotIn(secret, out)
        out = gm.scrub_log_line("reply Set-Cookie: sid=abc123; Path=/; HttpOnly")
        self.assertNotIn("abc123", out)

    def test_plain_line_unchanged(self):
        self.assertEqual(gm.scrub_log_line("Failed to renew certificate"), "Failed to renew certificate")


class ScrubSubprocessOutput(unittest.TestCase):
    """A failing check's stderr reaches the incident and the webhook, so it is scrubbed like a log line."""

    def _message(self, check, stderr, stdout=""):
        done = mock.Mock(returncode=1, stderr=stderr, stdout=stdout)
        with mock.patch.object(gm, "ensure_executable", return_value=("/bin/x", "")), \
                mock.patch.object(gm.subprocess, "run", return_value=done):
            ok, message = gm.execute_check(check)
        self.assertFalse(ok)
        return message

    def test_synthetic_stderr_is_scrubbed(self):
        message = self._message({"type": "synthetic", "script": "/bin/x"},
                                "login k6bp@example.com password=hunter2 https://x.example/a?k=1")
        for secret in ("k6bp@example.com", "hunter2", "x.example"):
            self.assertNotIn(secret, message)

    def test_certbot_failure_lines_are_scrubbed(self):
        message = self._message({"type": "certbot"}, "Failed to renew for k6bp@example.com token=abc123")
        for secret in ("k6bp@example.com", "abc123"):
            self.assertNotIn(secret, message)


if __name__ == "__main__":
    unittest.main()
