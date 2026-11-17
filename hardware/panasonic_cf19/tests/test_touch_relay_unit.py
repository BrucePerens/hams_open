# SPDX-License-Identifier: GPL-3.0-or-later
"""The touch relay unit restarts a crashed relay. Plain unittest: python3 -B tests/test_touch_relay_unit.py"""
import configparser
import os
import unittest

UNIT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "touch-relay", "panasonic-cf19-touch@.service")


class TouchRelayUnit(unittest.TestCase):
    def test_a_crashed_relay_is_restarted_while_the_device_is_present(self):
        unit = configparser.ConfigParser(interpolation=None, strict=False)
        unit.optionxform = str
        unit.read(UNIT)
        self.assertEqual(unit["Service"].get("Restart"), "on-failure")
        self.assertTrue(unit["Service"].get("RestartSec"))


if __name__ == "__main__":
    unittest.main()
