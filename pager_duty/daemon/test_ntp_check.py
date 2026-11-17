# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The "ntp" check type of generalized_monitor.py. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_ntp_check.py
"""
# Tests [@ANCHOR: pager_duty:daemon_execute_check]
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class NtpCheckTests(unittest.TestCase):
    def test_the_checks_own_port_is_the_one_queried(self):
        with mock.patch.object(gm.ntplib, "NTPClient") as client:
            client.return_value.request.return_value = mock.Mock(offset=0.001)
            ok, _msg = gm.execute_check({"type": "ntp", "target": "ntp.invalid", "port": 1123})
        self.assertTrue(ok)
        self.assertEqual(client.return_value.request.call_args.kwargs["port"], 1123)


if __name__ == "__main__":
    unittest.main()
