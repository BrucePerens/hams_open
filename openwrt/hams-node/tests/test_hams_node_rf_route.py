# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-rf-route with a recording stand-in for `ip`: RF routes are preferred (lower metric), tunnel routes are the
backup and the only routes in the tunnel-only table that marked (end-to-end encrypted) traffic uses, and nothing happens without prefixes."""

import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-rf-route"
sys.path.insert(0, str(HERE.parents[2] / "ham_tls"))
import ham_boundary  # noqa: E402

P1, P2 = "2001:db8:ab40::/60", "2001:db8:ab50::/60"


class RfRouteTests(unittest.TestCase):
    def run_script(self, link_type, prefixes, iface="br-rf0", action="apply"):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        log = root / "calls"
        fake = root / "bin" / "ip"
        fake.parent.mkdir()
        fake.write_text('#!/bin/sh\necho "$*" >> "%s"\n' % log)
        fake.chmod(0o755)
        src = root / "prefixes"
        src.write_text(prefixes)
        env = dict(os.environ, PATH="%s:%s" % (fake.parent, os.environ["PATH"]))
        result = subprocess.run(["sh", str(SCRIPT), action, iface, link_type, str(src)], env=env, capture_output=True, text=True)
        return result, (log.read_text().splitlines() if log.exists() else [])

    def test_rf_routes_get_the_low_metric_and_never_enter_the_tunnel_table(self):
        result, calls = self.run_script("rf", "# members\n%s\n%s\n" % (P1, P2))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ["-6 route replace %s dev br-rf0 metric 100" % P1, "-6 route replace %s dev br-rf0 metric 100" % P2])

    def test_tunnel_routes_are_the_backup_and_fill_the_tunnel_only_table(self):
        result, calls = self.run_script("internet", P1 + "\n", iface="wg-hams")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [
            "-6 route replace %s dev wg-hams metric 200" % P1,
            "-6 route replace %s dev wg-hams table 197" % P1,
            "-6 rule del fwmark 151 lookup 197 priority 89",
            "-6 rule add fwmark 151 lookup 197 priority 89",
        ])

    def test_the_boundary_mark_is_the_one_the_rule_matches(self):
        mark = ham_boundary.DEFAULTS["tunnel_mark"]
        self.assertEqual(mark, 151)
        self.assertIn("meta mark set %d" % mark, ham_boundary.render_ruleset())

    def test_down_removes_the_routes(self):
        result, calls = self.run_script("rf", P1 + "\n", action="down")
        self.assertEqual((result.returncode, calls), (0, ["-6 route del %s dev br-rf0 metric 100" % P1]))

    def test_nothing_is_done_without_prefixes(self):
        for kind in ("rf", "internet"):
            result, calls = self.run_script(kind, "# none yet\n\n")
            self.assertEqual((result.returncode, calls), (0, []))

    def test_unknown_link_type_and_bad_input_are_refused(self):
        result, calls = self.run_script("gre", P1 + "\n")
        self.assertEqual((result.returncode, calls), (1, []))
        result, calls = self.run_script("rf", "2001:db8::/60; reboot\n")
        self.assertEqual((result.returncode, calls), (1, []))
        result, calls = self.run_script("rf", P1 + "\n", iface="a b")
        self.assertEqual((result.returncode, calls), (1, []))


if __name__ == "__main__":
    unittest.main()
