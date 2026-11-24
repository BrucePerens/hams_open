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
    def render(self, secret="s3cret", rf_secrets=("4200000007", "4200000008"), deprecated="", **over):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "uci").chmod(0o755)
        (root / "rf.d").mkdir()
        for asn in rf_secrets:
            (root / "rf.d" / (asn + ".secret")).write_text("rfkey-" + asn + "\n")
        (root / "deplist").write_text(deprecated)
        cfg = dict(BASE, secret_file=str(root / "secret"), rf_secret_dir=str(root / "rf.d"), **over)
        (root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items()))
        if secret is not None:
            (root / "secret").write_text(secret + "\n")
        out = root / "bird.conf"
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root), HAMS_NODE_DEPRECATION_LIST=str(root / "deplist"))
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

    N1 = "4200000007,fe80::7,rf0,2001:db8:ab00:70::/60"
    N2 = "4200000008,fe80::8,rf1,2001:db8:ab00:20::/60,2001:db8:ab00:ffff::/96"

    def test_rf_neighbours_are_direct_sessions_preferred_over_route_servers(self):
        _, conf, _ = self.render(neighbor="%s %s" % (self.N1, self.N2))
        self.assertIn("protocol bgp hams_rf1 {", conf)
        self.assertIn("neighbor fe80::7%rf0 as 4200000007;", conf)
        self.assertIn("local as 4200000001;", conf.split("protocol bgp hams_rf1 {")[1])
        self.assertIn("neighbor fe80::8%rf1 as 4200000008;", conf)
        self.assertIn("bgp_local_pref = 300;", conf)

    def ah(self, action="print", **over):
        """Run `hams-node-bird ah ACTION` with a stand-in for ip; returns (result, the ip xfrm lines)."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "ip").write_text('#!/bin/sh\ncase "$1" in -6) echo "    inet6 fe80::aaaa/64 scope link";; xfrm) echo "$*" >> "$FAKE/ip.log";; esac\n')
        for n in ("uci", "ip"):
            (root / "bin" / n).chmod(0o755)
        (root / "rf.d").mkdir()
        for asn in ("4200000007", "4200000008"):
            (root / "rf.d" / (asn + ".secret")).write_text("rfkey-" + asn + "\n")
        (root / "deplist").write_text(over.pop("deprecated", ""))
        cfg = dict(BASE, rf_secret_dir=str(root / "rf.d"))
        cfg.update(dict({"neighbor": "4200000007,fe80::7,rf0,2001:db8:ab00:70::/60"}, **over))
        (root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items()))
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root), HAMS_NODE_DEPRECATION_LIST=str(root / "deplist"),
                   HAMS_NODE_STATE=str(root))
        self.state = root
        result = subprocess.run(["sh", str(SCRIPT), "ah", action], env=env, capture_output=True, text=True)
        log = root / "ip.log"
        return result, [ln for ln in (result.stdout.splitlines() if action == "print" else (log.read_text().splitlines() if log.exists() else [])) if ln]

    def test_rf_sessions_are_authenticated_with_ipsec_ah_never_encrypted(self):
        _, conf, _ = self.render(neighbor="%s %s" % (self.N1, self.N2))
        block = conf.split("protocol bgp hams_rf1 {")[1].split("\n}\n")[0]
        self.assertIn("connect retry time 15;", block)
        self.assertNotIn("password", block)  # the kernel has no TCP-MD5; AH below carries the authentication
        result, lines = self.ah()
        self.assertEqual(result.returncode, 0, result.stderr)
        adds = [ln for ln in lines if " state add " in ln or " policy add " in ln]
        self.assertEqual(len(adds), 4)
        self.assertTrue(all("proto ah" in ln or "tmpl" in ln for ln in adds))
        self.assertTrue(any("state add src fe80::aaaa dst fe80::7 proto ah spi 4200000001" in ln for ln in adds))
        self.assertTrue(any("state add src fe80::7 dst fe80::aaaa proto ah spi 4200000007" in ln for ln in adds))
        self.assertTrue(any("policy add src fe80::7/128 dst fe80::aaaa/128 proto tcp dir in" in ln for ln in adds))
        self.assertTrue(any("policy add src fe80::aaaa/128 dst fe80::7/128 proto tcp dir out" in ln for ln in adds))
        self.assertTrue(all("hmac(sha512)" in ln for ln in adds if "state add" in ln))
        self.assertNotIn("esp", "\n".join(lines))
        self.assertNotIn("enc ", "\n".join(lines))
        self.assertNotIn("rfkey", "\n".join(lines))  # the key is a hash of the secret, never the secret

    def test_ah_down_removes_and_other_asn_gets_another_key(self):
        _, lines = self.ah("down")
        self.assertTrue(lines and not any(" add " in ln for ln in lines))
        self.assertTrue(any("state delete" in ln for ln in lines) and any("policy delete" in ln for ln in lines))

    def test_rf_neighbours_are_the_gossip_neighbours_and_removed_on_down(self):
        self.ah("apply", neighbor="%s %s" % (self.N1, self.N2))
        self.assertEqual((self.state / "gossip_neighbours").read_text().split(), ["fe80::7%rf0", "fe80::8%rf1"])
        self.ah("down")
        self.assertEqual((self.state / "gossip_neighbours").read_text(), "")

    def test_ah_is_not_set_up_for_a_deprecated_neighbour(self):
        _, lines = self.ah(deprecated="4200000007 x\n")
        self.assertEqual(lines, [])

    def test_rf_neighbour_may_announce_only_its_own_60_and_siit_96(self):
        _, conf, _ = self.render(neighbor="%s %s" % (self.N1, self.N2))
        f1 = conf.split("filter hams_in_rf1 {")[1].split("\n}\n")[0]
        f2 = conf.split("filter hams_in_rf2 {")[1].split("\n}\n")[0]
        self.assertIn("net ~ [2001:db8:ab00:70::/60{60,64}]", f1)
        self.assertNotIn("2001:db8:ab00::/40", f1)
        self.assertIn("net ~ [2001:db8:ab00:20::/60{60,64}]", f2)
        self.assertIn("net = 2001:db8:ab00:ffff::/96", f2)
        self.assertNotIn("/96", f1)

    def test_rf_neighbours_work_without_route_servers_or_the_server_secret(self):
        result, conf, _ = self.render(secret=None, server="", neighbor=self.N1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("hams_rf1", conf)
        self.assertNotIn("hams_rs1", conf)

    def test_rf_neighbour_exports_only_the_own_prefix(self):
        _, conf, _ = self.render(neighbor=self.N1)
        out = conf.split("filter hams_out_rf {")[1].split("\n}\n")[0]
        self.assertIn("2001:db8:ab00:10::/60{60,64}", out)
        self.assertNotIn("bgp_large_community ~", out)
        self.assertIn("export filter hams_out_rf;", conf)

    def test_neighbour_without_a_key_or_with_a_loose_prefix_is_refused(self):
        for over in ({"neighbor": self.N1, "rf_secrets": ()}, {"neighbor": "4200000007,fe80::7,rf0"},
                     {"neighbor": "4200000007,fe80::7,rf0,2001:db8:ab00::/40"}):
            result, conf, _ = self.render(**over)
            self.assertNotEqual(result.returncode, 0, over)
            self.assertIsNone(conf)

    def test_a_deprecated_neighbour_is_not_configured(self):
        for dep in ("4200000007 x\n", "KEY 2001:db8:ab00:70::/60,2001:db8:ab00:71::/60\n"):
            result, conf, _ = self.render(deprecated=dep, neighbor="%s %s" % (self.N1, self.N2))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("neighbor fe80::7%rf0", conf)
            self.assertIn("neighbor fe80::8%rf1", conf)
            self.assertIn("deprecat", result.stderr)

    def test_bad_neighbour_refused(self):
        for bad in ("x,fe80::7,rf0,2001:db8:ab00:70::/60", "4200000007,fe80::7;,rf0,2001:db8:ab00:70::/60", "4200000007,2001:db8::7,rf0,2001:db8:ab00:70::/60",
                    "4200000007,fe80::7,rf 0,2001:db8:ab00:70::/60", "4200000007,fe80::7,rf0,2001:db8:ab00:70::/60;"):
            result, conf, _ = self.render(neighbor=bad)
            self.assertNotEqual(result.returncode, 0, bad)
            self.assertIsNone(conf)

    @unittest.skipUnless(shutil.which("bird") or os.path.exists("/usr/sbin/bird"), "bird is not installed")
    def test_bird_parses_the_generated_file(self):
        bird = shutil.which("bird") or "/usr/sbin/bird"
        for over in ({"rf_live": "0"}, {"rf_live": "1"}, {"neighbor": "%s %s" % (self.N1, self.N2)}):
            result, conf, out = self.render(**over)
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = subprocess.run([bird, "-p", "-c", str(out)], capture_output=True, text=True)
            self.assertEqual(parsed.returncode, 0, parsed.stdout + parsed.stderr)


if __name__ == "__main__":
    unittest.main()
