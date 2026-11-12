# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of the Power class of power_cycle_stress.py (no hardware: the shell and uhubctl calls are replaced)."""
import importlib.util
import os
import sys
import types
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location("power_cycle_stress", os.path.join(HERE, "power_cycle_stress.py"))
pcs = importlib.util.module_from_spec(SPEC)
sys.modules["power_cycle_stress"] = pcs
SPEC.loader.exec_module(pcs)


def args(**kw):
    base = dict(backend="none", location="2", port="1", uhubctl_extra="", sysfs_disable="/x", cmd_off="off-it", cmd_on="on-it")
    base.update(kw)
    return types.SimpleNamespace(**base)


class PowerTest(unittest.TestCase):
    # Tests [@ANCHOR: power_cycle_stress_power]
    def test_none_backend_does_nothing_and_succeeds(self):
        power = pcs.Power(args(backend="none"))
        self.assertEqual(power.off(), (True, ""))
        self.assertEqual(power.on(), (True, ""))

    # Tests [@ANCHOR: power_cycle_stress_power]
    def test_unknown_backend_is_a_failure(self):
        power = pcs.Power(args(backend="bogus"))
        self.assertEqual(power.off(), (False, "unknown backend"))
        self.assertEqual(power.on(), (False, "unknown backend"))

    # Tests [@ANCHOR: power_cycle_stress_power]
    def test_uhubctl_and_cmd_backends_build_the_command_and_report_the_exit_status(self):
        calls = []

        def fake_run(cmd, timeout):
            calls.append(cmd)
            return 0, "ok", ""

        original = pcs.run
        pcs.run = fake_run
        try:
            self.assertEqual(pcs.Power(args(backend="uhubctl", uhubctl_extra="-N")).off(), (True, "ok"))
            self.assertEqual(pcs.Power(args(backend="cmd")).on(), (True, "ok"))
        finally:
            pcs.run = original
        self.assertEqual(calls[0], ["uhubctl", "-l", "2", "-p", "1", "-a", "off", "-N"])
        self.assertEqual(calls[1], ["on-it"])


if __name__ == "__main__":
    unittest.main()
