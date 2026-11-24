# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-pool: the IPv4 pool for AREDN clients (allocation, refusals, Jool/nft/dnsmasq output) with recording stand-ins."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-pool"
GOOD = {"enabled": "1", "instance": "m1", "pool4": "10.20.30.96/28", "net_prefix": "2001:db8:ab", "iface": "eth0.2", "port": "80 8080"}


class PoolTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()
        self.map = self.root / "pool.map"
        self.dns = self.root / "dns" / "pool.conf"
        self.log = self.root / "log"

    def tool(self, name, body):
        p = self.root / "bin" / name
        p.write_text("#!/bin/sh\n" + body)
        p.chmod(0o755)

    def run_cmd(self, *args, **over):
        cfg = dict(GOOD, map=str(self.map), dns_out=str(self.dns), **over)
        lines = "\n".join('%s) echo "%s" ;;' % (k, v) for k, v in cfg.items())
        self.tool("uci", '[ "$1" = -q ] && [ "$2" = get ] || { echo "uci $*" >> "%s"; exit 0; }\nk=${3#hams_node_pool.main.}\ncase $k in\n%s\n*) exit 1 ;;\nesac\n' % (self.log, lines))
        for t in ("jool_siit", "ip", "dnsmasq", "firewall"):
            self.tool(t, 'echo "%s $*" >> "%s"\n' % (t, self.log))
        self.tool("nft", 'echo "nft $*" >> "%s"; cat >> "%s" 2>/dev/null\n' % (self.log, self.log))
        env = dict(os.environ, HAMS_NODE_POOL_FW=str(self.root / "pool-fw.nft"), PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), HAMS_NODE_INITD=str(self.root / "bin"))
        return subprocess.run(["sh", str(SCRIPT), *args], env=env, capture_output=True, text=True, stdin=subprocess.DEVNULL)

    def logtext(self):
        return self.log.read_text() if self.log.exists() else ""

    def test_add_allocates_the_next_free_pool_address_and_is_idempotent(self):
        r = self.run_cmd("add", "a.ham", "2001:db8:ab01::10")
        self.assertEqual((r.returncode, r.stdout.strip()), (0, "10.20.30.97"), r.stderr)
        self.assertEqual(self.run_cmd("add", "b.ham", "2001:db8:ab02::5").stdout.strip(), "10.20.30.98")
        self.assertEqual(self.run_cmd("add", "a.ham", "2001:db8:ab01::10").stdout.strip(), "10.20.30.97")
        self.assertEqual(self.run_cmd("add", "a.ham", "2001:db8:ab01::11").returncode, 1)  # same name, another target
        self.assertEqual(len(self.map.read_text().splitlines()), 2)

    def test_pool_runs_out_and_never_hands_out_network_or_broadcast(self):
        got = [self.run_cmd("add", "h%d.ham" % i, "2001:db8:ab01::%x" % i).stdout.strip() for i in range(1, 15)]
        self.assertEqual(got[0], "10.20.30.97")
        self.assertEqual(got[-1], "10.20.30.110")
        self.assertEqual(self.run_cmd("add", "h15.ham", "2001:db8:ab01::f").returncode, 1)

    def test_refusals(self):
        for args in (("add", "a.example.org", "2001:db8:ab01::1"), ("add", "A.ham", "2001:db8:ab01::1"), ("add", "a.ham", "2001:db9::1"),
                     ("add", "a.ham", "fd00::1"), ("add", "a.ham", "10.1.2.3"), ("add", "a;b.ham", "2001:db8:ab01::1")):
            self.assertEqual(self.run_cmd(*args).returncode, 1, args)
        for over in ({"pool4": "8.8.8.0/28"}, {"pool4": "10.20.30.96/16"}, {"pool4": "10.20.30.97/28"}, {"pool4": "0.0.0.0/24"}, {"instance": ""},
                     {"net_prefix": ""}, {"iface": ""}, {"port": "80;x"}, {"port": "80 443"}, {"port": "22"}, {"port": "8883"}):
            self.assertEqual(self.run_cmd("list", **over).returncode, 1, over)
        self.assertFalse(self.map.exists())

    def test_disabled_does_nothing(self):
        r = self.run_cmd("add", "a.ham", "2001:db8:ab01::1", enabled="0")
        self.assertEqual((r.returncode, r.stdout), (0, ""))
        self.assertFalse(self.map.exists())

    def test_apply_maps_in_jool_answers_in_dnsmasq_and_filters_ports(self):
        self.run_cmd("add", "a.ham", "2001:db8:ab01::10")
        self.map.write_text(self.map.read_text() + "evil.example.org 10.20.30.99 2001:db8:ab01::1\nout.ham 10.20.30.100 fd00::9\n")  # hand-edited rows are skipped
        r = self.run_cmd("apply")
        self.assertEqual(r.returncode, 0, r.stderr)
        log = self.logtext()
        self.assertIn("jool_siit -i m1 eamt add 10.20.30.97 2001:db8:ab01::10", log)
        self.assertIn("ip addr add 10.20.30.97/32 dev eth0.2", log)
        self.assertNotIn("10.20.30.99", log.split("table inet")[0])
        self.assertNotIn("fd00::9", log)
        self.assertIn("dnsmasq restart", log)
        self.assertEqual(self.dns.read_text().count("address=/"), 1)
        self.assertIn("address=/a.ham/10.20.30.97", self.dns.read_text())
        self.assertIn('ip daddr { 10.20.30.97 } iifname != "eth0.2" counter drop', log)
        self.assertIn("tcp dport { 80, 8080 } accept", log)
        self.assertIn("counter drop", log.split("accept")[1])  # everything else to the pool is dropped

    def test_apply_opens_only_the_translated_flows_in_the_firewall_and_clear_closes_them(self):
        self.run_cmd("add", "a.ham", "2001:db8:ab01::10")
        self.run_cmd("apply")
        self.assertEqual((self.root / "pool-fw.nft").read_text(),
                         'iifname "eth0.2" ip6 daddr { 2001:db8:ab01::10 } tcp dport { 80, 8080 } accept\nip saddr { 10.20.30.97 } tcp sport { 80, 8080 } accept\n')
        log = self.logtext()
        for want in ("uci -q set firewall.hams_pool=include", "uci -q set firewall.hams_pool.type=nftables", "uci -q set firewall.hams_pool.position=chain-pre",
                     "uci -q set firewall.hams_pool.chain=forward", "uci -q commit firewall", "firewall reload"):
            self.assertIn(want, log)
        self.run_cmd("clear")
        self.assertFalse((self.root / "pool-fw.nft").exists())
        self.assertIn("uci -q delete firewall.hams_pool", self.logtext())

    def test_clear_removes_everything_it_added(self):
        self.run_cmd("add", "a.ham", "2001:db8:ab01::10")
        self.run_cmd("apply")
        self.log.unlink()
        self.assertEqual(self.run_cmd("clear").returncode, 0)
        log = self.logtext()
        self.assertIn("jool_siit -i m1 eamt remove 10.20.30.97 2001:db8:ab01::10", log)
        self.assertIn("ip addr del 10.20.30.97/32 dev eth0.2", log)
        self.assertIn("nft delete table inet hams_pool", log)
        self.assertFalse(self.dns.exists())

    def test_del_drops_the_row(self):
        self.run_cmd("add", "a.ham", "2001:db8:ab01::10")
        self.run_cmd("add", "b.ham", "2001:db8:ab01::11")
        self.run_cmd("del", "a.ham")
        self.assertEqual(self.map.read_text().split()[0], "b.ham")


if __name__ == "__main__":
    unittest.main()
