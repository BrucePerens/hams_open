# SPDX-License-Identifier: GPL-2.0-or-later
"""The SIIT namespace scenario tool: its guest scripts have no unfilled placeholder and keep to short lines."""

import pathlib
import sys
import unittest

TOOLS = pathlib.Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import siit_netns_test as scenario  # noqa: E402


class ScenarioTests(unittest.TestCase):
    def test_pushed_script_exists(self):
        self.assertTrue((scenario.NODE / "usr" / "sbin" / "hams-node-siit").is_file())

    def test_placeholders_are_all_filled(self):
        for text in (scenario.SETUP, scenario.TRAFFIC, scenario.NEGATIVE):
            filled = scenario.fill(text)
            for marker in ("P96", "V4HOST", "V6HOST", "OTHERPREFIX"):
                self.assertNotIn(marker, filled)
        self.assertIn("2001:db8:ab01:1::a01:203", scenario.fill(scenario.TRAFFIC))

    def test_other_domain_is_not_the_loaded_one(self):
        self.assertIn("2001:db8:ab01:2::a01:203", scenario.fill(scenario.NEGATIVE))
        self.assertNotEqual(scenario.DOMAIN, scenario.OTHER_DOMAIN)

    def test_export_is_a_valid_registry_export(self):
        self.assertEqual(list(scenario.siit_map.gateway_configs(scenario.export())), ["mesha"])

    def test_guest_lines_are_short(self):
        for text in (scenario.ONLINE, scenario.SETUP, scenario.TRAFFIC, scenario.NEGATIVE):
            for line in scenario.fill(text).splitlines():
                self.assertLess(len(line), 330, line)


if __name__ == "__main__":
    unittest.main()
