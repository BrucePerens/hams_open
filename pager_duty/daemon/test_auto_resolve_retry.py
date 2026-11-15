# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A failed auto-resolve call is retried on the next passing pass. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_auto_resolve_retry.py
Nothing contacts a real service: the check, the RPC client and the clock are replaced.
"""
# Tests [@ANCHOR: pager_duty:auto_resolve]
# Tests [@ANCHOR: pager_duty:polling_thread]
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class Stop(Exception):
    pass


class AutoResolveRetry(unittest.TestCase):
    def run_passes(self, results, resolve_outcomes):
        """Runs polling_thread over `results` (True = pass); returns the number of auto_resolve calls."""
        passes = iter(results)
        outcomes = iter(resolve_outcomes)
        sleeps = []

        def sleep(seconds):
            sleeps.append(seconds)
            if len(sleeps) >= len(results):
                raise Stop

        absent = {"state": gm.pagerduty_maintenance.ABSENT, "error": ""}
        with mock.patch.object(gm, "execute_check", side_effect=lambda *_a, **_k: (next(passes), "x")), \
                mock.patch.object(gm, "record_check_status"), \
                mock.patch.object(gm, "report"), \
                mock.patch.object(gm, "is_in_maintenance", return_value=False), \
                mock.patch.object(gm, "parent_suppresses", return_value=False), \
                mock.patch.object(gm.pagerduty_maintenance, "read_flag", return_value=absent), \
                mock.patch.object(gm.time, "sleep", side_effect=sleep), \
                mock.patch.object(gm, "auto_resolve", side_effect=lambda *_a, **_k: next(outcomes)) as resolve:
            self.addCleanup(gm.FAILING_CHECKS.clear)
            self.addCleanup(gm.FAILING_NAMES.clear)
            with self.assertRaises(Stop):
                gm.polling_thread(mock.Mock(), {"id": 9101, "name": "Retry Check", "type": "system", "interval": 60})
        return resolve.call_count

    def test_a_failed_resolve_is_retried_on_the_next_pass_and_then_stops(self):
        # Passes 3 and 4 call it (the third fails, the fourth works); passes 5 and 6 do not.
        self.assertEqual(self.run_passes([True] * 6, [False, True]), 2)

    def test_a_new_failure_starts_a_new_resolve(self):
        # three passes resolve; a failure, then three passes resolve again.
        self.assertEqual(self.run_passes([True, True, True, False, True, True, True], [True, True]), 2)


if __name__ == "__main__":
    unittest.main()
