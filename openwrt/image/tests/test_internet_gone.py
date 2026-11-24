# SPDX-License-Identifier: GPL-2.0-or-later
"""The internet-gone scenario tool: the scripts it pushes into the guests exist, and its guest scripts keep to short lines."""

import pathlib
import sys
import unittest

TOOLS = pathlib.Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import internet_gone_test as scenario  # noqa: E402


class ScenarioTests(unittest.TestCase):
    def test_pushed_scripts_exist(self):
        for name in ("hams-node-bird", "hams-node-zone", "hams-node-time", "hams-node-wg"):
            self.assertTrue((scenario.SBIN / name).is_file(), name)
        self.assertTrue((scenario.SBIN.parents[1] / "etc" / "init.d" / "hams-node-bird").is_file())

    def test_guest_scripts_have_lines_the_serial_console_accepts(self):
        for text in (scenario.ONLINE, scenario.BIRD_SETUP, scenario.ZONE_SETUP, scenario.DEPRECATION_CHECK, scenario.ROGUE, scenario.ROGUE_CHECK):
            for line in text.splitlines():
                self.assertLess(len(line), 200, line)

    def test_the_two_nodes_have_distinct_asns_and_prefixes(self):
        a, b = scenario.NODES["A"], scenario.NODES["B"]
        self.assertNotEqual(a["asn"], b["asn"])
        self.assertNotEqual(a["p60"], b["p60"])
        self.assertNotEqual(a["mac"], b["mac"])


if __name__ == "__main__":
    unittest.main()
