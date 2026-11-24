# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-web (AREDN web gateway): refusals, the ham_proxy command line, the dnsmasq line, and the init script."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-web"
INITD = HERE.parent / "files" / "etc" / "init.d" / "hams-node-web"
GOOD = {"enabled": "1", "listen_addr": "10.20.30.1", "port": "8443", "callsign": "W1AW", "resolver": "[2001:db8::53]:53",
        "cert": "/c.pem", "key": "/k.pem", "log": "/var/log/hams-node-web/stationid.log", "whitelist": "", "whitelist_key": "", "sign_key": "",
        "dns_out": ""}
HARNESS = """
procd_open_instance() { echo "procd_open_instance" >> "$FAKE/log"; }
procd_close_instance() { echo "procd_close_instance" >> "$FAKE/log"; }
procd_set_param() { echo "procd_set_param $*" >> "$FAKE/log"; }
procd_add_reload_trigger() { echo "trigger $*" >> "$FAKE/log"; }
. "$SCRIPT"
"$@"
"""


class WebTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()

    def tool(self, name, body):
        p = self.root / "bin" / name
        p.write_text("#!/bin/sh\n" + body)
        p.chmod(0o755)

    def uci(self, **over):
        cfg = dict(GOOD, **over)
        lines = "\n".join('%s) echo "%s" ;;' % (k, v) for k, v in cfg.items() if v != "")
        self.tool("uci", 'k=${2#hams_node_web.main.}; [ "$1" = -q ] && k=${3#hams_node_web.main.}\ncase $k in\n%s\n*) exit 1 ;;\nesac\n' % lines)

    def env(self, **extra):
        return dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root), SCRIPT=str(INITD), **extra)

    def run_script(self, *args, **over):
        self.uci(**over)
        return subprocess.run(["sh", str(SCRIPT), *args], env=self.env(), capture_output=True, text=True)

    def test_args_are_the_gateway_command_line_on_the_mesh_address(self):
        r = self.run_script("args")
        self.assertEqual(r.returncode, 0, r.stderr)
        a = r.stdout.split("\n")
        self.assertEqual(a[a.index("--listen") + 1], "10.20.30.1:8443")
        self.assertIn("--gateway", a)
        self.assertEqual(a[a.index("--callsign") + 1], "W1AW")
        self.assertEqual(a[a.index("--ham-resolver") + 1], "[2001:db8::53]:53")
        self.assertIn("--whitelist", a)  # no signed list: only .ham names
        self.assertNotIn("--allow-private-upstream", a)  # the lab flag never comes from the package
        # ham_proxy's own flags (daemons/ham_proxy/README.md); --ham-resolver arrives with hams_open #3328
        known = {"--gateway", "--listen", "--cert", "--key", "--callsign", "--log", "--ham-resolver", "--whitelist", "--signed-whitelist", "--whitelist-key", "--sign-key"}
        self.assertLessEqual({x for x in a if x.startswith("--")}, known)

    def test_signed_list_and_sign_key_when_present(self):
        (self.root / "wl.json").write_text("{}")
        (self.root / "wk.hex").write_text("00")
        r = self.run_script("args", whitelist=str(self.root / "wl.json"), whitelist_key=str(self.root / "wk.hex"), sign_key="/s.pem")
        a = r.stdout.split("\n")
        self.assertEqual(a[a.index("--signed-whitelist") + 1], str(self.root / "wl.json"))
        self.assertNotIn("--whitelist", a)
        self.assertEqual(a[a.index("--sign-key") + 1], "/s.pem")

    def test_refuses_public_wildcard_or_missing_settings(self):
        for over in ({"listen_addr": "0.0.0.0"}, {"listen_addr": "8.8.8.8"}, {"listen_addr": "10.1.2.3;x"}, {"listen_addr": ""},
                     {"listen_addr": "172.32.0.1"}, {"callsign": ""}, {"callsign": "W1AW;rm"}, {"resolver": ""}, {"port": "80x"}):
            r = self.run_script("args", **over)
            self.assertEqual(r.returncode, 1, over)
            self.assertEqual(r.stdout, "")

    def test_disabled_does_nothing(self):
        r = self.run_script("args", enabled="0")
        self.assertEqual((r.returncode, r.stdout), (0, ""))

    def test_dns_answers_ham_with_the_gateway_address(self):
        out = self.root / "d" / "web.conf"
        r = self.run_script("dns", str(out))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("address=/ham/10.20.30.1\n", out.read_text())

    def test_init_starts_proxy_and_dnsmasq_and_stop_removes_dns(self):
        self.tool("hams-node-web", 'case $1 in args) printf "%s\\n" --gateway --listen 10.20.30.1:8443 ;; dns) echo "address=/ham/10.20.30.1" > "$2" ;; esac\n')
        self.tool("uci", "echo /tmp/x/log\n")
        self.tool("dnsmasq", 'echo "dnsmasq $*" >> "$FAKE/log"\n')
        dns = self.root / "dns.conf"
        env = self.env(HAMS_NODE_INITD=str(self.root / "bin"), HAMS_NODE_WEB_DNS=str(dns))
        r = subprocess.run(["sh", "-c", HARNESS, "sh", "start_service"], env=env, capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        log = (self.root / "log").read_text().splitlines()
        self.assertIn("dnsmasq restart", log)
        self.assertIn("procd_set_param command /usr/sbin/ham_proxy --gateway --listen 10.20.30.1:8443", log)
        self.assertTrue(dns.exists())
        subprocess.run(["sh", "-c", HARNESS, "sh", "stop_service"], env=env, capture_output=True, text=True)
        self.assertFalse(dns.exists())

    def test_init_refuses_when_the_config_is_refused(self):
        self.tool("hams-node-web", "exit 1\n")
        r = subprocess.run(["sh", "-c", HARNESS, "sh", "start_service"], env=self.env(), capture_output=True, text=True)
        self.assertEqual(r.returncode, 1)
        self.assertFalse((self.root / "log").exists())


if __name__ == "__main__":
    unittest.main()
