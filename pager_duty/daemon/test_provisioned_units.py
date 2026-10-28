# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The "provisioned units are in operation" check (check_provisioned_units.py). Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_provisioned_units.py
"""
# Tests [@ANCHOR: pager_duty:provisioned_units]
# Tests [@ANCHOR: pager_duty:units_not_in_operation]
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_provisioned_units as check  # noqa: E402


class FakeInfra:
    MANIFEST = {"static_files": [
        {"path": "/opt/hams/systemd/a.service", "environments": ["prod"], "content": "Type=oneshot\n"},
        {"path": "/opt/hams/systemd/a.timer", "environments": ["prod"], "content": "[Timer]\n"},
        {"path": "/opt/hams/systemd/daemon.service", "environments": ["prod"], "content": "Type=simple\n"},
        {"path": "/opt/hams/systemd/paid.timer", "environments": ["prod"], "content": "", "opt_in": "spends money"},
        {"path": "/opt/hams/systemd/tmpl@.service", "environments": ["prod"], "content": ""},
        {"path": "/opt/hams/systemd/test-only.service", "environments": ["test"], "content": ""},
        {"path": "/etc/hosts", "environments": ["prod"], "content": "x"},
        {"path": "/opt/hams/systemd/hostclass.service", "environments": ["prod"], "content": "", "host_class": "ca_signer"},
    ]}

    @staticmethod
    def _in_host_class(spec):
        return not spec.get("host_class")


def shown(**states):
    def show(unit):
        return states.get(unit, {"LoadState": "loaded", "ActiveState": "active", "UnitFileState": "enabled"})
    return show


class ExpectedUnitsTests(unittest.TestCase):
    def test_only_prod_unit_files_for_this_host_and_not_templates(self):
        units = check.expected_units(FakeInfra)
        self.assertEqual(sorted(units), ["a.service", "a.timer", "daemon.service", "paid.timer"])
        self.assertTrue(units["a.service"]["oneshot"])
        self.assertFalse(units["daemon.service"]["oneshot"])
        self.assertTrue(units["paid.timer"]["opt_in"])


class NotInOperationTests(unittest.TestCase):
    def setUp(self):
        self.units = check.expected_units(FakeInfra)

    def problems(self, **states):
        return dict(check.units_not_in_operation(self.units, show=shown(**states)))

    def test_all_running_is_healthy(self):
        self.assertEqual(self.problems(), {})

    def test_a_unit_systemd_does_not_know_is_not_installed(self):
        self.assertEqual(self.problems(**{"a.timer": {"LoadState": "not-found"}}), {"a.timer": "not installed"})

    def test_a_stopped_timer_and_a_stopped_long_running_service_are_reported(self):
        got = self.problems(**{"a.timer": {"LoadState": "loaded", "ActiveState": "inactive"},
                               "daemon.service": {"LoadState": "loaded", "ActiveState": "failed"}})
        self.assertEqual(got, {"a.timer": "not running", "daemon.service": "not running"})

    def test_a_timer_driven_oneshot_may_be_inactive(self):
        self.assertEqual(self.problems(**{"a.service": {"LoadState": "loaded", "ActiveState": "inactive"}}), {})

    def test_an_opt_in_timer_that_is_only_linked_is_left_alone_but_an_enabled_one_must_run(self):
        linked = {"LoadState": "loaded", "ActiveState": "inactive", "UnitFileState": "linked"}
        self.assertEqual(self.problems(**{"paid.timer": linked}), {})
        enabled = dict(linked, UnitFileState="enabled")
        self.assertEqual(self.problems(**{"paid.timer": enabled}), {"paid.timer": "not running"})

    def test_a_service_whose_condition_is_unmet_on_this_host_is_not_a_fault(self):
        self.assertEqual(self.problems(**{"daemon.service": {"LoadState": "loaded", "ActiveState": "inactive", "ConditionResult": "no"}}), {})

    def test_an_expected_off_unit_is_skipped(self):
        got = check.units_not_in_operation(self.units, show=shown(**{"a.timer": {"LoadState": "not-found"}}), expected_off={"a.timer"})
        self.assertEqual(got, [])


class MainTests(unittest.TestCase):
    def run_main(self, problems, units=None):
        with mock.patch.object(check, "infrastructure", FakeInfra), \
                mock.patch.object(check, "units_not_in_operation", return_value=problems), \
                mock.patch("sys.stdout") as out, mock.patch("sys.stderr") as err:
            code = check.main([])
        return code, "".join(c.args[0] for c in out.write.call_args_list), "".join(c.args[0] for c in err.write.call_args_list)

    def test_healthy_exits_zero(self):
        code, out, _ = self.run_main([])
        self.assertEqual(code, 0)
        self.assertIn("in operation", out)

    def test_a_problem_pages_with_a_short_first_line_and_the_full_list(self):
        problems = [(f"u{i}.timer", "not running") for i in range(6)]
        code, _, err = self.run_main(problems)
        self.assertEqual(code, 1)
        self.assertIn("SEVERITY:high", err)
        self.assertIn("6 provisioned unit(s) not in operation: u0.timer, u1.timer, u2.timer, u3.timer +2", err)
        self.assertIn("u5.timer (not running)", err)

    def test_an_unloadable_manifest_fails_loudly(self):
        with mock.patch.object(check, "expected_units", side_effect=KeyError("static_files")), \
                mock.patch("sys.stderr") as err:
            code = check.main([])
        self.assertEqual(code, 1)
        self.assertIn("MANIFEST did not load", "".join(c.args[0] for c in err.write.call_args_list))


if __name__ == "__main__":
    unittest.main()
