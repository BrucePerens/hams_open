# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of the CF-19 on-screen-keyboard GNOME Shell extension, run by Node (test_osk_extension_harness.cjs) against the real
extension.js with stand-ins for GNOME's own modules.

    python3 -B tests/test_osk_extension.py
"""
import os
import shutil
import subprocess
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
EXTENSION = os.path.join(os.path.dirname(HERE), "gnome-shell", "cf19-osk@hams.com", "extension.js")
HARNESS = os.path.join(HERE, "test_osk_extension_harness.cjs")


@unittest.skipUnless(shutil.which("node"), "node is not installed")
class TestOskExtension(unittest.TestCase):
    def run_harness(self, name):
        result = subprocess.run(["node", HARNESS, EXTENSION, name], capture_output=True, text=True, timeout=60, check=False)
        self.assertEqual(result.returncode, 0, f"{name} failed:\n{result.stdout}\n{result.stderr}")

    def test_enable_binds_the_key_and_owns_the_bus_name(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_enable]
        # Tests [@ANCHOR: panasonic_cf19:js_osk_extension_class]
        self.run_harness("enable")

    def test_disable_undoes_enable_and_switches_the_setting_off_in_laptop_position(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_disable]
        self.run_harness("disable")

    def test_read_tablet_reads_the_switch(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_read_tablet]
        self.run_harness("read_tablet")

    def test_set_enabled_writes_the_setting_only_when_it_changes(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_set_enabled]
        self.run_harness("set_enabled")

    def test_on_visibility_switches_the_setting_off_when_the_laptop_keyboard_hides(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_on_visibility]
        self.run_harness("on_visibility")

    def test_toggle_hides_a_shown_keyboard_and_shows_a_hidden_one(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_toggle]
        self.run_harness("toggle")

    def test_show_enables_the_setting_and_opens_the_keyboard(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_show]
        self.run_harness("show")

    def test_hide_closes_the_keyboard(self):
        # Tests [@ANCHOR: panasonic_cf19:js_osk_hide]
        self.run_harness("hide")


if __name__ == "__main__":
    unittest.main()
