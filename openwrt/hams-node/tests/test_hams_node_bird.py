# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-bird with a stand-in for uci (reads KEY=VALUE lines)."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-bird"
UCI = '#!/bin/sh\nkey=${3#hams_node_bird.main.}\ngrep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-\n'
BASE = {"enabled": "1", "asn": "4200000001", "prefix": "2001:db8:ab00::/40", "prefix60": "2001:db8:ab00:10::/60", "serial": "16909060",
        "rf_live": "0", "server": "2001:db8:ab00::1 2001:db8:ab00::2"}


class BirdTests(unittest.TestCase):
    def render(self, secret="s3cret", **over):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "uci").chmod(0o755)
        cfg = dict(BASE, secret_file=str(root / "secret"), **over)
        (root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items()))
        if secret is not None:
            (root / "secret").write_text(secret + "\n")
        out = root / "bird.conf"
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root))
        result = subprocess.run(["sh", str(SCRIPT), "render", str(out)], env=env, capture_output=True, text=True)
        return result, (out.read_text() if out.exists() else None), out

    def test_sessions_and_filters_match_the_router_config_format(self):
        result, conf, out = self.render()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("router id 1.2.3.4;", conf)
        self.assertIn("route 2001:db8:ab00:10::/60 unreachable;", conf)
        self.assertEqual(conf.count("protocol bgp hams_rs"), 2)
        self.assertIn("local 2001:db8:ab00:10::1 as 4200000001;", conf)
        self.assertIn("neighbor 2001:db8:ab00::1 as 4200000000;", conf)
        self.assertIn("neighbor 2001:db8:ab00::2 as 4200000000;", conf)
        self.assertIn('password "s3cret";', conf)
        self.assertIn("net ~ [2001:db8:ab00:10::/60{60,64}]", conf)
        self.assertEqual(oct(out.stat().st_mode & 0o777), "0o600")

    def test_rf_first_local_pref(self):
        _, conf, _ = self.render()
        self.assertIn("bgp_large_community ~ [(4200000000, 1, 1)] then bgp_local_pref = 200; else bgp_local_pref = 100;", conf)

    def test_own_routes_tagged_only_while_rf_live(self):
        self.assertNotIn("bgp_large_community.add", self.render(rf_live="0")[1])
        self.assertIn("bgp_large_community.add((4200000000, 1, 1));", self.render(rf_live="1")[1])

    def test_disabled_or_no_servers_writes_nothing(self):
        for over in ({"enabled": "0"}, {"server": ""}):
            result, conf, _ = self.render(**over)
            self.assertEqual(result.returncode, 0)
            self.assertIsNone(conf)

    def test_bad_input_refused(self):
        for over in ({"asn": "4200000001; evil"}, {"server": "2001:db8::1 x;y"}, {"prefix60": "2001:db8:ab00:10::/48"}):
            result, conf, _ = self.render(**over)
            self.assertNotEqual(result.returncode, 0, over)
            self.assertIsNone(conf)
        result, conf, _ = self.render(secret='a"b')
        self.assertNotEqual(result.returncode, 0)
        result, conf, _ = self.render(secret=None)
        self.assertNotEqual(result.returncode, 0)

    def test_rf_neighbours_are_direct_sessions_preferred_over_route_servers(self):
        _, conf, _ = self.render(neighbor="4200000007,fe80::7,rf0 4200000008,2001:db8:ab00:20::1,rf1,2001:db8:ab00:20::/60")
        self.assertIn("protocol bgp hams_rf1 {", conf)
        self.assertIn("neighbor fe80::7%rf0 as 4200000007;", conf)
        self.assertIn("neighbor 2001:db8:ab00:20::1%rf1 as 4200000008;", conf)
        self.assertIn("bgp_local_pref = 300;", conf)
        # the RF filter only accepts the neighbour's own /60 when it is configured
        self.assertIn("net ~ [2001:db8:ab00:20::/60{60,64}]", conf)

    def test_rf_sessions_are_not_authenticated_or_encrypted(self):
        _, conf, _ = self.render(neighbor="4200000007,fe80::7,rf0")
        block = conf.split("protocol bgp hams_rf1 {")[1].split("\n}\n")[0]
        self.assertNotIn("password", block)
        self.assertNotIn("authentication", block)

    def test_rf_neighbours_work_without_route_servers(self):
        result, conf, _ = self.render(server="", neighbor="4200000007,fe80::7,rf0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("hams_rf1", conf)
        self.assertNotIn("hams_rs1", conf)

    def test_rf_neighbours_work_without_the_bgp_secret(self):
        result, conf, _ = self.render(secret=None, server="", neighbor="4200000007,fe80::7,rf0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("hams_rf1", conf)

    def test_rf_neighbour_exports_own_prefix_and_rf_learned_routes_only(self):
        _, conf, _ = self.render(neighbor="4200000007,fe80::7,rf0")
        self.assertIn("filter hams_out_rf {", conf)
        out = conf.split("filter hams_out_rf {")[1].split("\n}\n")[0]
        self.assertIn("bgp_large_community.add((4200000000, 1, 1));", out)
        self.assertIn("2001:db8:ab00:10::/60{60,64}", out)
        self.assertIn("export filter hams_out_rf;", conf)

    def test_bad_neighbour_refused(self):
        for bad in ("x,fe80::7,rf0", "4200000007,fe80::7;,rf0", "4200000007,fe80::7,rf 0", "4200000007,fe80::7", "4200000007,fe80::7,rf0,2001:db8::/48;"):
            result, conf, _ = self.render(neighbor=bad)
            self.assertNotEqual(result.returncode, 0, bad)
            self.assertIsNone(conf)

    @unittest.skipUnless(shutil.which("bird") or os.path.exists("/usr/sbin/bird"), "bird is not installed")
    def test_bird_parses_the_generated_file(self):
        bird = shutil.which("bird") or "/usr/sbin/bird"
        for over in ({"rf_live": "0"}, {"rf_live": "1"}, {"neighbor": "4200000007,fe80::7,rf0 4200000008,2001:db8:ab00:20::1,rf1,2001:db8:ab00:20::/60"}):
            result, conf, out = self.render(**over)
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = subprocess.run([bird, "-p", "-c", str(out)], capture_output=True, text=True)
            self.assertEqual(parsed.returncode, 0, parsed.stdout + parsed.stderr)


if __name__ == "__main__":
    unittest.main()
