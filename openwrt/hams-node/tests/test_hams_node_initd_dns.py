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
        out = self.root / "dns" / "hams.conf"
        self.assertTrue(out.exists())
        self.run_fn("hams-node-dns", "stop_service")
        self.assertFalse(out.exists())

    def test_trigger_and_procd(self):
        result, log = self.run_fn("hams-node-dns", "service_triggers")
        self.assertEqual(log, ["trigger hams_node_dns"])
        self.assertIn("USE_PROCD=1", (INITD / "hams-node-dns").read_text())


if __name__ == "__main__":
    unittest.main()
