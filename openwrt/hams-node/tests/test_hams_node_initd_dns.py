# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the boot-time init scripts of hams-node-dns: sourced with procd stand-ins and recording tools."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
INITD = HERE.parent / "files" / "etc" / "init.d"
HARNESS = """
procd_open_instance() { echo "procd_open_instance" >> "$FAKE/log"; }
procd_close_instance() { echo "procd_close_instance" >> "$FAKE/log"; }
procd_set_param() { echo "procd_set_param $*" >> "$FAKE/log"; }
procd_add_reload_trigger() { echo "trigger $*" >> "$FAKE/log"; }
. "$SCRIPT"
"$@"
"""


class InitTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()

    def tool(self, name, body):
        p = self.root / "bin" / name
        p.write_text("#!/bin/sh\n" + body)
        p.chmod(0o755)

    def run_fn(self, script, fn, **env):
        env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root), SCRIPT=str(INITD / script),
                   HAMS_NODE_ETC=str(self.root / "etc"), HAMS_NODE_INITD=str(self.root / "bin"), HAMS_NODE_DNS_OUT=str(self.root / "dns" / "hams.conf"), **env)
        (self.root / "log").unlink(missing_ok=True)
        result = subprocess.run(["sh", "-c", HARNESS, "sh", fn], env=env, capture_output=True, text=True)
        log = self.root / "log"
        return result, log.read_text().splitlines() if log.exists() else []

    def test_dns_renders_and_reloads_dnsmasq_and_stop_removes(self):
        self.tool("hams-node-dns", 'mkdir -p "$(dirname "$2")"; echo "server=/ham/2001:db8::53" > "$2"\n')
        self.tool("uci", 'echo "uci $*" >> "$FAKE/uci.log"\n[ "$1" = -q ] && [ "$2" = get ] && [ "$3" = "dhcp.@dnsmasq[0]" ] && echo dnsmasq\nexit 0\n')
        self.tool("dnsmasq", 'echo "dnsmasq $*" >> "$FAKE/log"\n')
        result, log = self.run_fn("hams-node-dns", "start_service")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, ["dnsmasq restart"])
        # dnsmasq only reads a directory named in dhcp.@dnsmasq[0].confdir: it is set to where the snippet goes
        uci = (self.root / "uci.log").read_text()
        self.assertIn("uci set dhcp.@dnsmasq[0].confdir=%s" % (self.root / "dns"), uci)
        self.assertIn("uci commit dhcp", uci)
        self.assertIn("uci add_list dhcp.@dnsmasq[0].rebind_domain=ham", uci)  # private answers for .ham names pass rebind protection
        out = self.root / "dns" / "hams.conf"
        self.assertTrue(out.exists())
        self.run_fn("hams-node-dns", "stop_service")
        self.assertFalse(out.exists())

    def v6_tools(self, resolvers):
        self.tool("hams-node-dns", 'mkdir -p "$(dirname "$2")"; : > "$2"\n')
        self.tool("dnsmasq", "exit 0\n")
        self.tool("ip", 'echo "ip $*" >> "$FAKE/log"\n')
        self.tool("nft", 'echo "nft $*" >> "$FAKE/log"; touch "$FAKE/nft.rules"; [ "$1" = -f ] && cat >> "$FAKE/nft.rules"\n')
        self.tool("uci", 'case "$3" in dhcp.@dnsmasq[0]) echo dnsmasq;; dhcp.@dnsmasq[0].confdir) echo /x;;'
                  ' hams_node_dns.main.v6_addr) echo fd00:db8:1::53;; hams_node_dns.main.resolver) echo "%s";; esac\nexit 0\n' % resolvers)

    def test_v6_address_is_sent_to_first_ipv6_resolver_without_touching_dnsmasq(self):
        self.v6_tools("192.0.2.1 2001:db8::53#5353 2001:db8::54")
        result, log = self.run_fn("hams-node-dns", "start_service")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ip -6 addr add fd00:db8:1::53/128 dev br-lan", log)
        rules = (self.root / "nft.rules").read_text()
        self.assertIn("ip6 daddr fd00:db8:1::53 meta l4proto { udp, tcp } th dport 53 dnat ip6 to [2001:db8::53]:5353", rules)
        self.assertIn("ct status dnat masquerade", rules)
        self.assertNotIn("filter", rules)  # only a nat table: no firewall change, AREDN's dnsmasq keeps its own settings
        self.run_fn("hams-node-dns", "stop_service")
        self.assertIn("nft delete table inet hams_dns6", (self.root / "log").read_text())

    def test_v6_needs_an_ipv6_resolver(self):
        self.v6_tools("192.0.2.1")
        result, _ = self.run_fn("hams-node-dns", "start_service")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("IPv6 resolver", result.stderr)
        self.assertNotIn("dnat", (self.root / "nft.rules").read_text())

    def test_trigger_and_procd(self):
        result, log = self.run_fn("hams-node-dns", "service_triggers")
        self.assertEqual(log, ["trigger hams_node_dns"])
        self.assertIn("USE_PROCD=1", (INITD / "hams-node-dns").read_text())


if __name__ == "__main__":
    unittest.main()
