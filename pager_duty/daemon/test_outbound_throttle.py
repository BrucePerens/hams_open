# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The webhook and the SMTP fallback of generalized_monitor.py do not repeat on every failing pass. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_outbound_throttle.py
Nothing here contacts a real service: urlopen and SMTP are replaced.
"""
# Tests [@ANCHOR: daemon_report_incident]
# Tests [@ANCHOR: pager_duty:fallback_notify]
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class OutboundThrottle(unittest.TestCase):
    def setUp(self):
        gm.LAST_OUTBOUND.clear()
        self.addCleanup(gm.LAST_OUTBOUND.clear)
        env = mock.patch.dict(
            os.environ,
            {"PAGER_WEBHOOK_URL": "https://webhook.invalid/x", "PAGER_FALLBACK_EMAIL": "a@example.invalid", "SMTP_HOST": "smtp.invalid"},
        )
        env.start()
        self.addCleanup(env.stop)
        absent = {"state": gm.pagerduty_maintenance.ABSENT, "error": ""}
        flag = mock.patch.object(gm.pagerduty_maintenance, "read_flag", return_value=absent)
        flag.start()
        self.addCleanup(flag.stop)
        self.flag = flag

    def test_a_failing_source_posts_the_webhook_once_not_every_pass(self):
        with mock.patch.object(gm.urllib.request, "urlopen") as urlopen:
            for _ in range(5):
                gm.report(mock.Mock(), "Disk", "full")
            gm.report(mock.Mock(), "Other", "full")
        self.assertEqual(urlopen.call_count, 2)

    def test_a_higher_severity_is_not_held_back_by_an_earlier_alert(self):
        with mock.patch.object(gm.urllib.request, "urlopen") as urlopen:
            gm.report(mock.Mock(), "Disk", "filling", severity="medium")
            gm.report(mock.Mock(), "Disk", "full", severity="critical")
        self.assertEqual(urlopen.call_count, 2)

    def test_a_webhook_that_failed_is_tried_again_next_pass(self):
        with mock.patch.object(gm.urllib.request, "urlopen", side_effect=OSError("down")) as urlopen:
            gm.report(mock.Mock(), "Disk", "full")
            gm.report(mock.Mock(), "Disk", "full")
        self.assertEqual(urlopen.call_count, 2)

    def test_a_failure_after_recovery_notifies_at_once(self):
        with mock.patch.object(gm.urllib.request, "urlopen") as urlopen:
            gm.report(mock.Mock(), "Disk", "full")
            self.assertTrue(gm.auto_resolve(mock.Mock(), "Disk"))
            gm.report(mock.Mock(), "Disk", "full again")
        self.assertEqual(urlopen.call_count, 2)

    def test_the_webhook_posts_again_after_the_repeat_interval(self):
        with mock.patch.object(gm.urllib.request, "urlopen") as urlopen, mock.patch.object(gm.time, "time") as now:
            now.return_value = 1000.0
            gm.report(mock.Mock(), "Disk", "full")
            now.return_value = 1000.0 + gm.OUTBOUND_REPEAT_SECONDS + 1
            gm.report(mock.Mock(), "Disk", "full")
        self.assertEqual(urlopen.call_count, 2)

    def test_no_webhook_during_maintenance(self):
        self.flag.stop()
        active = {"state": gm.pagerduty_maintenance.ACTIVE, "error": ""}
        with mock.patch.object(gm.pagerduty_maintenance, "read_flag", return_value=active), mock.patch.object(
            gm.urllib.request, "urlopen"
        ) as urlopen:
            gm.report(mock.Mock(), "Disk", "full")
        urlopen.assert_not_called()
        self.flag.start()

    def test_rpc_failure_sends_the_smtp_fallback_once_not_every_pass(self):
        client = mock.Mock()
        client.execute.side_effect = ConnectionError("odoo down")
        with mock.patch.object(gm.smtplib, "SMTP") as smtp, mock.patch.object(gm.urllib.request, "urlopen"):
            for _ in range(4):
                gm.report(client, "Disk", "full")
        self.assertEqual(smtp.call_count, 1)


if __name__ == "__main__":
    unittest.main()
