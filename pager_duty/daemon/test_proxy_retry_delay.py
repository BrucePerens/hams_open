# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The Redis proxy loops of the generalized monitor back off during an outage instead of retrying every second. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_proxy_retry_delay.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor  # noqa: E402


class ProxyRetryDelay(unittest.TestCase):
    def test_proxy_retry_delay_backs_off_and_caps(self):
        # Tests [@ANCHOR: pager_duty:proxy_retry_delay]
        # Tests [@ANCHOR: pager_duty:test_proxy_retry_delay_backs_off_and_caps]
        delays = [generalized_monitor.proxy_retry_delay(n) for n in range(1, 11)]
        self.assertEqual(delays, [1, 2, 4, 8, 16, 32, 60, 60, 60, 60])
        # An hour-long outage logs a handful of warnings per minute-scale window, not 3600.
        self.assertLess(sum(1 for _ in self._attempts_within(3600)), 70)

    @staticmethod
    def _attempts_within(seconds):
        elapsed, failures = 0, 0
        while elapsed < seconds:
            failures += 1
            elapsed += generalized_monitor.proxy_retry_delay(failures)
            yield failures


if __name__ == "__main__":
    unittest.main()
