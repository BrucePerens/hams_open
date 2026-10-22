# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The "Systemd Failed Services Tracker" check (type systemd, target "*") of generalized_monitor.py. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_failed_units.py
"""
# Tests [@ANCHOR: pager_duty:parse_failed_unit_names]
# Tests [@ANCHOR: pager_duty:failed_units_report]
import os
import subprocess
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402

LISTING = (
    "hams-exception-report@club.crawl.service.service loaded failed failed Report the failure of club.crawl.service\n"
    "fwupd-refresh.service loaded failed failed Refresh fwupd metadata\n"
    "foo.service loaded failed failed Foo\n"
)
SHOW = {
    "hams-exception-report@club.crawl.service.service": "Result=exit-code\nStateChangeTimestamp=Sun 2026-10-18 18:24:17 UTC\n",
    "foo.service": "Result=signal\nStateChangeTimestamp=Mon 2026-10-19 01:02:03 UTC\n",
    "a@1.service": "Result=exit-code\nStateChangeTimestamp=Mon 2026-10-19 01:02:03 UTC\n",
    "a@2.service": "Result=exit-code\nStateChangeTimestamp=Mon 2026-10-19 01:02:04 UTC\n",
}


def fake_run(listing):
    def run(cmd, **_kw):
        done = mock.Mock(returncode=0, stderr="")
        if "list-units" in cmd:
            done.stdout = listing
        else:
            done.stdout = SHOW.get(cmd[2], "")
        return done
    return run


class FailedUnitsCheckTests(unittest.TestCase):
    def check(self, listing, **extra):
        check = {"type": "systemd", "target": "*", "ignored_services": "fwupd-refresh.service, NetworkManager-wait-online.service"}
        check.update(extra)
        with mock.patch.object(gm, "ensure_executable", return_value=("/bin/systemctl", "")), \
                mock.patch.object(gm.subprocess, "run", side_effect=fake_run(listing)):
            return gm.execute_check(check)

    def test_no_failed_units_passes(self):
        ok, msg = self.check("")
        self.assertTrue(ok, msg)

    def test_only_ignored_units_passes(self):
        ok, msg = self.check("fwupd-refresh.service loaded failed failed x\n")
        self.assertTrue(ok, msg)

    def test_the_message_names_each_unit_with_its_result_and_when_it_failed(self):
        ok, msg = self.check(LISTING)
        self.assertFalse(ok)
        self.assertIn("2 failed systemd unit(s)", msg)
        self.assertIn("hams-exception-report@club.crawl.service.service (exit-code since 2026-10-18 18:24:17 UTC)", msg)
        self.assertIn("foo.service (signal since 2026-10-19 01:02:03 UTC)", msg)
        self.assertNotIn("fwupd-refresh", msg)
        self.assertIn("reset-failed", msg)

    def test_a_cleared_unit_makes_the_next_run_pass(self):
        self.assertFalse(self.check(LISTING)[0])
        self.assertTrue(self.check("fwupd-refresh.service loaded failed failed x\n")[0])

    def test_instances_of_one_template_are_one_entry(self):
        ok, msg = self.check("a@1.service loaded failed failed A\na@2.service loaded failed failed A\n")
        self.assertFalse(ok)
        self.assertIn("1 failed systemd unit(s)", msg)
        self.assertIn("2 instances of a@: a@1.service, a@2.service", msg)

    def test_a_unit_listed_twice_is_counted_once(self):
        self.assertEqual(gm.parse_failed_unit_names("x.service l f f\nx.service l f f\n"), ["x.service"])

    def test_a_pattern_ignores_every_instance_of_a_template(self):
        self.assertEqual(gm.parse_failed_unit_names("a@1.service l f f\nb.service l f f\n", ["a@*"]), ["b.service"])

    def test_a_bullet_in_the_listing_is_not_part_of_the_name(self):
        self.assertEqual(gm.parse_failed_unit_names("● x.service loaded failed failed X\n"), ["x.service"])

    def test_a_failing_systemctl_show_still_names_the_unit(self):
        def run(cmd, **_kw):
            if "list-units" in cmd:
                return mock.Mock(returncode=0, stdout="x.service l f f\n", stderr="")
            raise subprocess.TimeoutExpired(cmd, 10)
        with mock.patch.object(gm, "ensure_executable", return_value=("/bin/systemctl", "")), \
                mock.patch.object(gm.subprocess, "run", side_effect=run):
            ok, msg = gm.execute_check({"type": "systemd", "target": "*"})
        self.assertFalse(ok)
        self.assertIn("x.service", msg)

    def test_a_systemctl_error_is_a_failed_check(self):
        with mock.patch.object(gm, "ensure_executable", return_value=("/bin/systemctl", "")), \
                mock.patch.object(gm.subprocess, "run", return_value=mock.Mock(returncode=1, stdout="", stderr="boom")):
            ok, msg = gm.execute_check({"type": "systemd", "target": "*"})
        self.assertFalse(ok)
        self.assertIn("boom", msg)


class FailingUnitsAreSentWithTheIncidentTests(unittest.TestCase):
    """Tests [@ANCHOR: pager_duty:check_units]: the units behind a failure travel with the incident, so the server
    joins the one ticket per unit (hams_helpdesk's exception fingerprint) instead of opening one per check."""

    def run_check(self, check, listing="", is_active=None):
        def run(cmd, **_kw):
            if "is-active" in cmd:
                state = (is_active or {}).get(cmd[2], "active")
                return mock.Mock(returncode=0 if state == "active" else 3, stdout=state + "\n", stderr="")
            return fake_run(listing)(cmd)
        gm.CHECK_UNITS.clear()
        with mock.patch.object(gm, "ensure_executable", return_value=("/bin/systemctl", "")), \
                mock.patch.object(gm.subprocess, "run", side_effect=run):
            return gm.execute_check(check)

    def test_the_failed_units_tracker_sends_every_failed_unit_once(self):
        check = {"type": "systemd", "target": "*", "id": 7, "ignored_services": "fwupd-refresh.service"}
        ok, _msg = self.run_check(check, LISTING)
        self.assertFalse(ok)
        self.assertEqual(
            gm.CHECK_UNITS[7],
            ["foo.service", "hams-exception-report@club.crawl.service.service"],
        )

    def test_a_named_services_check_sends_the_units_that_are_not_active(self):
        check = {"type": "systemd", "target": "a.service, b.service, c.service", "id": 8}
        ok, _msg = self.run_check(check, is_active={"b.service": "failed", "c.service": "activating"})
        self.assertFalse(ok)
        self.assertEqual(gm.CHECK_UNITS[8], ["b.service", "c.service"])

    def test_a_passing_check_sends_no_units(self):
        check = {"type": "systemd", "target": "*", "id": 9}
        self.assertTrue(self.run_check(check, "")[0])
        self.assertNotIn(9, gm.CHECK_UNITS)

    def test_report_puts_the_units_in_the_incident_payload(self):
        client = mock.Mock()
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("PAGER_WEBHOOK_URL", None)
            gm.report(client, "Tracker", "1 failed", "high", units=["fcc.uls.sync.service"])
            gm.report(client, "Disk", "full", "high")
        first, second = (call.kwargs["vals"] for call in client.execute.call_args_list)
        self.assertEqual(first["units"], ["fcc.uls.sync.service"])
        self.assertNotIn("units", second)


if __name__ == "__main__":
    unittest.main()
