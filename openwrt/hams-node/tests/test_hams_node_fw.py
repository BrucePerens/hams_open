# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-fw (firewall zones, rf interface registry) and its use by hams-node-wg, with a stand-in for uci."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SBIN = HERE.parent / "files" / "usr" / "sbin"
UCI = """#!/bin/sh
if [ "$1" = batch ]; then cat >> "$FAKE/batch"; exit 0; fi
if [ "$1" = commit ]; then echo "$*" >> "$FAKE/commits"; exit 0; fi
if [ "$2" = show ]; then cat "$FAKE/firewall_show" 2>/dev/null; exit 0; fi
if [ "$1" = add_list ]; then
    echo "$*" >> "$FAKE/addlist"; kv=${2#hams_node_fw.main.}; k=${kv%%=*}; v=${kv#*=}
    if grep -q "^$k=" "$FAKE/uci"; then sed -i "s|^$k=\\(.*\\)|$k=\\1 $v|; s|^$k= |$k=|" "$FAKE/uci"; else echo "$k=$v" >> "$FAKE/uci"; fi
    exit 0
fi
key=${3#hams_node_fw.main.}
key=${key#hams_node.main.}
grep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-
"""
BASE = {"enabled": "1", "iface": "wg-hams", "rf_iface": "rf0", "open": "tcp:8080 tcp:22"}


class Env:
    def __init__(self, test, **over):
        tmp = tempfile.TemporaryDirectory()
        test.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()
        for name, text in (("uci", UCI), ("wg", '#!/bin/sh\necho "$*" >> "$FAKE/wgcalls"\n'), ("ip", "#!/bin/sh\nexit 0\n")):
            (self.root / "bin" / name).write_text(text)
            (self.root / "bin" / name).chmod(0o755)
        (self.root / "uci").write_text("".join("%s=%s\n" % kv for kv in dict(BASE, **over).items()))
        # hams-node-fw as found on PATH, the way it is installed next to hams-node-wg
        (self.root / "bin" / "hams-node-fw").symlink_to(SBIN / "hams-node-fw")
        self.env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root))

    def run(self, *args):
        return subprocess.run(["sh", *map(str, args)], env=self.env, capture_output=True, text=True)

    def read(self, name):
        p = self.root / name
        return p.read_text().splitlines() if p.exists() else []


class FwTests(unittest.TestCase):
    def test_zones_default_closed_and_open_port_gets_a_rule(self):
        env = Env(self)
        result = env.run(SBIN / "hams-node-fw", "render")
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        for z in ("hams_zone", "hams_rf_zone"):
            self.assertIn("set firewall.%s.input=DROP" % z, lines)
            self.assertIn("set firewall.%s.forward=DROP" % z, lines)
        self.assertIn("set firewall.hams_in_tcp_8080.dest_port=8080", lines)
        self.assertIn("set firewall.hams_in_tcp_8080.src=ham", lines)
        self.assertIn("set firewall.hams_in_tcp_8080.target=ACCEPT", lines)
        self.assertNotIn("set firewall.hams_zone.input=ACCEPT", lines)

    def test_no_open_ports_means_no_rules(self):
        lines = Env(self, open="").run(SBIN / "hams-node-fw", "render").stdout.splitlines()
        self.assertEqual([l for l in lines if "=rule" in l and "hams_in_" in l], [])
        self.assertIn("set firewall.hams_zone.input=DROP", lines)

    def test_encrypted_service_port_never_opened_toward_rf(self):
        result = Env(self).run(SBIN / "hams-node-fw", "render")
        lines = result.stdout.splitlines()
        self.assertIn("set firewall.hams_in_tcp_22=rule", lines)  # the tunnel side may
        self.assertNotIn("set firewall.hams_rf_in_tcp_22=rule", lines)
        self.assertIn("set firewall.hams_rf_in_tcp_8080=rule", lines)
        self.assertIn("not opened toward RF", result.stderr)

    def test_disabled_and_bad_input(self):
        self.assertEqual(Env(self, enabled="0").run(SBIN / "hams-node-fw", "render").stdout, "")
        for over in ({"open": "tcp:80;x"}, {"iface": "a b;c"}, {"rf_iface": "wg-hams"}):
            result = Env(self, **over).run(SBIN / "hams-node-fw", "render")
            self.assertNotEqual(result.returncode, 0, over)

    def test_apply_commits(self):
        env = Env(self)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "apply").returncode, 0)
        self.assertIn("set firewall.hams_zone.name=ham", env.read("batch"))
        self.assertEqual(env.read("commits"), ["commit firewall"])

    def test_is_rf(self):
        env = Env(self)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "is-rf", "rf0").returncode, 0)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "is-rf", "wg-hams").returncode, 1)


FIREWALL_SHOW = """firewall.lan=zone
firewall.lan.name=lan
firewall.hams_zone=zone
firewall.hams_zone.name=ham
firewall.hams_rf_zone=zone
firewall.hams_rf_zone.name=hamrf
firewall.hams_in_tcp_8080=rule
firewall.hams_in_tcp_8080.name=hams-open-tcp-8080
firewall.hams_rf_in_udp_5000=rule
firewall.my_hams_in_x=rule
firewall.hams_mine=zone
"""


class RemoveTests(unittest.TestCase):
    def test_remove_deletes_only_the_sections_it_created(self):
        env = Env(self)
        (env.root / "firewall_show").write_text(FIREWALL_SHOW)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "remove").returncode, 0)
        batch = env.read("batch")
        self.assertEqual(sorted(batch), sorted([
            "delete firewall.hams_zone", "delete firewall.hams_rf_zone",
            "delete firewall.hams_in_tcp_8080", "delete firewall.hams_rf_in_udp_5000"]))
        self.assertEqual(env.read("commits"), ["commit firewall"])

    def test_remove_is_idempotent_when_nothing_is_left(self):
        env = Env(self)
        (env.root / "firewall_show").write_text("firewall.lan=zone\n")
        self.assertEqual(env.run(SBIN / "hams-node-fw", "remove").returncode, 0)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "remove").returncode, 0)
        self.assertEqual(env.read("batch"), [])
        self.assertEqual(env.read("commits"), [])

    def test_remove_works_when_the_fw_config_is_disabled(self):
        env = Env(self, enabled="0")
        (env.root / "firewall_show").write_text(FIREWALL_SHOW)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "remove").returncode, 0)
        self.assertIn("delete firewall.hams_zone", env.read("batch"))

    def test_apply_while_disabled_removes_stale_zones(self):
        env = Env(self, enabled="0")
        (env.root / "firewall_show").write_text(FIREWALL_SHOW)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "apply").returncode, 0)
        self.assertIn("delete firewall.hams_zone", env.read("batch"))
        self.assertEqual(env.read("commits"), ["commit firewall"])

    def test_enabled_when_enrolled_unless_switched_off(self):
        run = lambda env: env.run(SBIN / "hams-node-fw", "render").stdout
        self.assertIn("set firewall.hams_zone.name=ham", run(Env(self, enabled="", prefix60="2001:db8:ab40:10::/60")))
        self.assertEqual(run(Env(self, enabled="", prefix60="")), "")
        self.assertEqual(run(Env(self, enabled="0", prefix60="2001:db8:ab40:10::/60")), "")

    def test_lan_reaches_ham_but_ham_never_reaches_lan(self):
        lines = Env(self).run(SBIN / "hams-node-fw", "render").stdout.splitlines()
        self.assertIn("set firewall.hams_fwd_lan_ham=forwarding", lines)
        self.assertIn("set firewall.hams_fwd_lan_ham.src=lan", lines)
        self.assertIn("set firewall.hams_fwd_lan_ham.dest=ham", lines)
        self.assertEqual([l for l in lines if l.endswith("=forwarding")], ["set firewall.hams_fwd_lan_ham=forwarding"])
        for z in ("ham", "hamrf"):
            sec = "hams_drop_%s_lan" % z
            self.assertIn("set firewall.%s=rule" % sec, lines)
            self.assertIn("set firewall.%s.src=%s" % (sec, z), lines)
            self.assertIn("set firewall.%s.dest=lan" % sec, lines)
            self.assertIn("set firewall.%s.target=DROP" % sec, lines)
        # "list open" only ever opens the router's own input (no dest on an open rule)
        self.assertEqual([l for l in lines if l.startswith("set firewall.hams_in_") and ".dest=" in l and "dest_port" not in l], [])

    def test_ensure_iface_puts_ham_and_rf_interfaces_in_zones(self):
        env = Env(self, iface="", rf_iface="")
        self.assertEqual(env.run(SBIN / "hams-node-fw", "ensure", "wg-hams").returncode, 0)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "ensure", "-r", "rf0").returncode, 0)
        self.assertIn("add_list hams_node_fw.main.iface=wg-hams", env.read("addlist"))
        self.assertIn("add_list hams_node_fw.main.rf_iface=rf0", env.read("addlist"))
        self.assertIn("set firewall.hams_zone.name=ham", env.read("batch"))

    def test_ensure_refuses_wireguard_name_on_rf_and_is_idempotent(self):
        env = Env(self)
        self.assertNotEqual(env.run(SBIN / "hams-node-fw", "ensure", "rf0").returncode, 0)
        self.assertEqual(env.run(SBIN / "hams-node-fw", "ensure", "wg-hams").returncode, 0)
        self.assertEqual(env.read("addlist"), [])

    def test_check_fails_for_a_wireguard_interface_in_no_zone(self):
        env = Env(self)
        (env.root / "bin" / "wg").write_text('#!/bin/sh\necho "wg-hams wg-ondemand"\n')
        result = env.run(SBIN / "hams-node-fw", "check")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("wg-ondemand", result.stderr)
        self.assertNotIn("wg-hams", result.stderr)


class WireguardZoneOnApply(unittest.TestCase):
    def test_wg_apply_puts_its_interface_in_the_ham_zone(self):
        env = Env(self, iface="")
        (env.root / "peers").write_text("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= 192.0.2.7:51820 2001:db8:ab01::/48\n")
        self.assertEqual(env.run(SBIN / "hams-node-wg", "apply", "wg-hams", "internet", env.root / "peers").returncode, 0)
        self.assertIn("add_list hams_node_fw.main.iface=wg-hams", env.read("addlist"))

    def test_rf_route_apply_on_rf_puts_the_interface_in_hamrf(self):
        env = Env(self, rf_iface="")
        (env.root / "pfx").write_text("2001:db8:ab01::/48\n")
        self.assertEqual(env.run(SBIN / "hams-node-rf-route", "apply", "rf0", "rf", env.root / "pfx").returncode, 0)
        self.assertIn("add_list hams_node_fw.main.rf_iface=rf0", env.read("addlist"))


class WireguardRefusedOnRfInterface(unittest.TestCase):
    def test_wg_refused_on_an_interface_set_to_rf_even_if_called_as_internet(self):
        env = Env(self)
        peers = env.root / "peers"
        peers.write_text("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= 192.0.2.7:51820 2001:db8:ab01::/48\n")
        result = env.run(SBIN / "hams-node-wg", "apply", "rf0", "internet", peers)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("rf", result.stderr)
        self.assertEqual(env.read("wgcalls"), [])

    def test_wg_still_works_on_a_tunnel_interface(self):
        env = Env(self)
        peers = env.root / "peers"
        peers.write_text("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= 192.0.2.7:51820 2001:db8:ab01::/48\n")
        self.assertEqual(env.run(SBIN / "hams-node-wg", "apply", "wg-hams", "internet", peers).returncode, 0)
        self.assertEqual(len(env.read("wgcalls")), 1)


if __name__ == "__main__":
    unittest.main()
