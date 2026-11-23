# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-wg with a recording stand-in for the `wg` tool."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-wg"
KEY = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="


class WgTests(unittest.TestCase):
    def run_script(self, link_type, peers, iface="wg-hams", action="apply"):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        log = root / "calls"
        fake = root / "bin" / "wg"
        fake.parent.mkdir()
        fake.write_text('#!/bin/sh\necho "$*" >> "%s"\n' % log)
        fake.chmod(0o755)
        fake_ip = fake.parent / "ip"
        fake_ip.write_text('#!/bin/sh\necho "ip $*" >> "%s"\n' % log)
        fake_ip.chmod(0o755)
        src = root / "peers"
        src.write_text(peers)
        env = dict(os.environ, PATH="%s:%s" % (fake.parent, os.environ["PATH"]))
        result = subprocess.run(["sh", str(SCRIPT), action, iface, link_type, str(src)], env=env, capture_output=True, text=True)
        calls = log.read_text().splitlines() if log.exists() else []
        return result, calls

    def test_adds_each_peer(self):
        peers = "# route servers\n%s [2001:db8::1]:51820 2001:db8:ab00::/48\n\n%s 192.0.2.7:51820 2001:db8:ab01::/48,2001:db8:ab02::/48\n" % (KEY, KEY)
        result, calls = self.run_script("internet", peers)
        self.assertEqual(result.returncode, 0, result.stderr)
        wg_calls = [c for c in calls if not c.startswith("ip ")]
        self.assertEqual(wg_calls, [
            "set wg-hams peer %s endpoint [2001:db8::1]:51820 allowed-ips 2001:db8:ab00::/48" % KEY,
            "set wg-hams peer %s endpoint 192.0.2.7:51820 allowed-ips 2001:db8:ab01::/48,2001:db8:ab02::/48" % KEY,
        ])

    def test_policy_rules_below_priority_100_are_added_per_prefix(self):
        peers = "%s 192.0.2.7:51820 2001:db8:ab01::/48,10.1.0.0/16\n" % KEY
        result, calls = self.run_script("internet", peers)
        self.assertEqual(result.returncode, 0, result.stderr)
        ip_calls = [c for c in calls if c.startswith("ip ") and " rule add " in c]
        self.assertEqual(ip_calls, [
            "ip -6 rule add to 2001:db8:ab01::/48 lookup main priority 90",
            "ip -4 rule add to 10.1.0.0/16 lookup main priority 90",
        ])

    def test_down_removes_the_rules_and_the_peer(self):
        peers = "%s 192.0.2.7:51820 2001:db8:ab01::/48\n" % KEY
        result, calls = self.run_script("internet", peers, action="down")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ip -6 rule del to 2001:db8:ab01::/48 lookup main priority 90", calls)
        self.assertFalse([c for c in calls if " rule add " in c])
        self.assertIn("set wg-hams peer %s remove" % KEY, calls)

    def test_rf_link_is_refused_before_any_peer(self):
        for link_type in ("rf", "", "mesh"):
            result, calls = self.run_script(link_type, "%s 192.0.2.7:51820 2001:db8::/48\n" % KEY)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(calls, [])

    def test_malformed_lines_are_skipped_and_reported(self):
        peers = "onlykey\n%s 192.0.2.7:1 ::/0; reboot\n%s 192.0.2.8:51820 2001:db8::/48\n" % (KEY, KEY)
        result, calls = self.run_script("internet", peers)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("set wg-hams peer %s endpoint 192.0.2.8:51820 allowed-ips 2001:db8::/48" % KEY, calls)
        self.assertEqual(len([c for c in calls if c.startswith("set ")]), 1)

    def test_bad_interface_name_is_refused(self):
        result, calls = self.run_script("internet", "%s 192.0.2.7:1 ::/0\n" % KEY, iface="wg0; id")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_bad_characters_are_skipped_and_reported(self):
        result, calls = self.run_script("internet", "%s 192.0.2.7:1 10.0.0.0/8;id\n" % KEY)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])
        self.assertIn("bad characters", result.stderr)

    def test_ipv6_endpoint_brackets_are_allowed(self):
        result, calls = self.run_script("internet", "%s [2001:db8::1]:51820 ::/0\n" % KEY)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(len([c for c in calls if c.startswith("set ")]), 1)


if __name__ == "__main__":
    unittest.main()
