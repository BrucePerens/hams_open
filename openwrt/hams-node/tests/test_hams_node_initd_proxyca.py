# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the hams-node-proxyca init script and `args`: sourced with procd stand-ins, the real hams-node-proxyca, a stand-in uci."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
INITD = HERE.parent / "files" / "etc" / "init.d"
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-proxyca"
HARNESS = """
procd_open_instance() { echo "procd_open_instance" >> "$FAKE/log"; }
procd_close_instance() { echo "procd_close_instance" >> "$FAKE/log"; }
procd_set_param() { echo "procd_set_param $*" >> "$FAKE/log"; }
procd_add_reload_trigger() { echo "trigger $*" >> "$FAKE/log"; }
. "$SCRIPT"
"$@"
"""
UCI = '#!/bin/sh\n[ "$1" = -q ] && shift\nkey=${2#hams_node_proxyca.main.}\ngrep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-\n'
GOOD = {"enabled": "1", "proxy": "192.168.1.1:8443", "callsign": "W1AW", "resolver": "10.1.1.53:53", "gateway": "gw.example.net:8443",
        "gateway_ca": "/etc/hams-node/gw.pem", "net_prefix": "2001:db8:ab00:"}


@unittest.skipUnless(shutil.which("openssl"), "needs the openssl command line tool")
class InitTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()
        (self.root / "www").mkdir()
        (self.root / "bin" / "uci").write_text(UCI)
        (self.root / "bin" / "uci").chmod(0o755)
        shutil.copy(SCRIPT, self.root / "bin" / "hams-node-proxyca")
        (self.root / "whitelist").write_text("example.org\n")

    def run_fn(self, fn, **over):
        cfg = dict(GOOD, whitelist=str(self.root / "whitelist"), log=str(self.root / "logs" / "stationid.log"), **over)
        (self.root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items() if kv[1] is not None))
        env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root), SCRIPT=str(INITD / "hams-node-proxyca"),
                   HAMS_PROXY_DIR=str(self.root / "proxy"), HAMS_PAC_DIR=str(self.root / "www"))
        (self.root / "log").unlink(missing_ok=True)
        result = subprocess.run(["sh", "-c", HARNESS, "sh", fn], env=env, capture_output=True, text=True)
        log = self.root / "log"
        return result, log.read_text().splitlines() if log.exists() else []

    def test_off_by_default_runs_nothing_and_makes_no_ca(self):
        result, log = self.run_fn("start_service", enabled="0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, [])
        self.assertFalse((self.root / "proxy").exists())

    def test_enabled_makes_the_ca_and_pac_and_runs_ham_proxy_lan_on_the_lan_address(self):
        result, log = self.run_fn("start_service")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / "proxy" / "ca.pem").exists())
        self.assertEqual(oct((self.root / "proxy" / "ca.key").stat().st_mode & 0o777), "0o600")
        self.assertIn("PROXY 192.168.1.1:8443", (self.root / "www" / "hams.pac").read_text())
        self.assertEqual(log[0], "procd_open_instance")
        command = [l for l in log if l.startswith("procd_set_param command")][0].split()
        self.assertEqual(command[2], "/usr/sbin/ham_proxy")
        args = command[3:]
        self.assertEqual(args[0], "--lan")
        self.assertEqual(args[args.index("--listen") + 1], "192.168.1.1:8443")
        self.assertEqual(args[args.index("--upstream-gateway") + 1], "gw.example.net:8443")
        self.assertEqual(args[args.index("--callsign") + 1], "W1AW")
        self.assertNotIn("0.0.0.0", " ".join(args))
        self.assertIn("procd_set_param respawn", log)
        self.assertTrue((self.root / "logs").is_dir(), "the station-ID log directory is made")

    def test_reload_trigger_is_the_config(self):
        _, log = self.run_fn("service_triggers")
        self.assertEqual(log, ["trigger hams_node_proxyca"])

    def test_only_a_private_lan_address_is_ever_listened_on(self):
        for bad in ("0.0.0.0:8443", "[::]:8443", "8.8.8.8:8443", "203.0.113.5:8443", "192.168.1.1", "172.32.0.1:8443", ":8443", "127.1.2.3:8443"):
            result, log = self.run_fn("start_service", proxy=bad)
            self.assertNotEqual(result.returncode, 0, bad)
            self.assertEqual(log, [], bad)
        for ok in ("10.1.2.3:8443", "172.16.0.1:3128", "192.168.0.1:8443", "[fd12:3456::1]:8443"):
            result, log = self.run_fn("start_service", proxy=ok)
            self.assertEqual(result.returncode, 0, "%s: %s" % (ok, result.stderr))
            self.assertIn("procd_open_instance", log)

    def test_incomplete_configuration_refuses_to_start(self):
        for key in ("callsign", "resolver", "gateway", "gateway_ca"):
            result, log = self.run_fn("start_service", **{key: ""})
            self.assertNotEqual(result.returncode, 0, key)
            self.assertEqual(log, [], key)
        result, log = self.run_fn("start_service", callsign="W1AW;reboot")
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
