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

    @unittest.skipUnless(shutil.which("bird") or os.path.exists("/usr/sbin/bird"), "bird is not installed")
    def test_bird_parses_the_generated_file(self):
        bird = shutil.which("bird") or "/usr/sbin/bird"
        for over in ({"rf_live": "0"}, {"rf_live": "1"}):
            result, conf, out = self.render(**over)
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = subprocess.run([bird, "-p", "-c", str(out)], capture_output=True, text=True)
            self.assertEqual(parsed.returncode, 0, parsed.stdout + parsed.stderr)


if __name__ == "__main__":
    unittest.main()
