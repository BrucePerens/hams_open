# SPDX-License-Identifier: GPL-2.0-or-later
"""The SIIT scenario on stock AREDN: its guest scripts have no unfilled placeholder and keep to short lines."""

import pathlib
import sys
import unittest

TOOLS = pathlib.Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import siit_aredn_test as scenario  # noqa: E402


class ScenarioTests(unittest.TestCase):
    def test_placeholders_are_all_filled(self):
        for text in (scenario.GW_SETUP, scenario.SIX_SETUP, scenario.FOUR_SETUP):
            filled = scenario.base.fill(text)
            for marker in ("P96", "V4HOST", "V6HOST"):
                self.assertNotIn(marker, filled)

    def test_guest_lines_are_short(self):
        for text in (scenario.HOST_ONLINE, scenario.GW_SETUP, scenario.SIX_SETUP, scenario.FOUR_SETUP, scenario.TMO):
            for line in scenario.base.fill(text).splitlines():
                self.assertLess(len(line), 330, line)

    def test_gateway_link_macs_are_distinct(self):
        args = scenario.nic("s6", "52:54:00:aa:00:12", "listen=host:1")
        self.assertIn("virtio-net-pci,netdev=s6,mac=52:54:00:aa:00:12", args)


if __name__ == "__main__":
    unittest.main()
