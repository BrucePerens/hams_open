# SPDX-License-Identifier: GPL-2.0-or-later
"""The on-demand scenario tool: the scripts it pushes exist and its guest scripts keep to short lines."""

import pathlib
import sys
import unittest

TOOLS = pathlib.Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import ondemand_test as scenario  # noqa: E402


class ScenarioTests(unittest.TestCase):
    def test_pushed_scripts_exist(self):
        self.assertTrue((scenario.NODE / "usr" / "sbin" / "hams-node-ondemand").is_file())
        self.assertTrue((scenario.NODE / "etc" / "init.d" / "hams-node-ondemand").is_file())

    def test_guest_scripts_have_lines_the_serial_console_accepts(self):
        for text in (scenario.ONLINE, scenario.FETCH, scenario.SETUP, scenario.KEYS, scenario.MEM):
            for line in text.splitlines():
                self.assertLess(len(line), 330, line)
