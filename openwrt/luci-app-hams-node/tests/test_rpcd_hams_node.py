# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the luci-app-hams-node rpcd backend and its ACL, with stand-ins for uci and nft."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
PKG = HERE.parent
PLUGIN = PKG / "root" / "usr" / "libexec" / "rpcd" / "hams-node"
ACL = PKG / "root" / "usr" / "share" / "rpcd" / "acl.d" / "luci-app-hams-node.json"
MENU = PKG / "root" / "usr" / "share" / "luci" / "menu.d" / "luci-app-hams-node.json"
SECRET = "SECRETTOKENVALUE123"
KEY_A = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="


class Harness:
    def __init__(self, test):
        tmp = tempfile.TemporaryDirectory()
        test.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.bin = self.root / "bin"
        self.state = self.root / "state"
        self.ondemand = self.root / "ondemand"
        self.enroll_out = self.root / "enroll.out"
        self.calls = self.root / "calls"
        for d in (self.bin, self.state, self.ondemand):
            d.mkdir()
        self.uci = {}
        self.counters = {}
        self.boundary = True

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(0o755)

    def call(self, method, stdin="{}"):
        uci_lines = "\n".join('[ "$2" = "%s" ] && echo "%s" && exit 0' % (k, v) for k, v in self.uci.items())
        self.stub("uci", 'echo "uci $*" >> "%s"\n[ "$1" = -q ] && shift\n%s\nexit 1' % (self.calls, uci_lines))
        nft = ['echo "nft $*" >> "%s"' % self.calls]
        if self.boundary:
            for name, (p, b) in self.counters.items():
                nft.append('[ "$2 $3 $4" = "counter inet hams_boundary" ] && [ "$5" = %s ] && '
                           'echo "table inet hams_boundary { counter %s { packets %d bytes %d } }" && exit 0' % (name, name, p, b))
            nft.append('[ "$1" = list ] && [ "$2" = table ] && exit 0')
            nft.append("exit 1")
        else:
            nft.append("exit 1")
        self.stub("nft", "\n".join(nft))
        self.stub("hams-node-enroll", 'echo "enroll $*" >> "%s"\necho "On hams.com open https://hams.com/my/ipv6 and enter the code: ABCD-EFGH"' % self.calls)
        env = dict(os.environ, PATH="%s:%s" % (self.bin, os.environ["PATH"]), HAMS_STATE_DIR=str(self.state),
                   HAMS_ONDEMAND_DIR=str(self.ondemand), HAMS_ENROLL_OUT=str(self.enroll_out), HAMS_NO_BACKGROUND="1")
        out = subprocess.run(["sh", str(PLUGIN), "call", method], input=stdin, env=env, capture_output=True, text=True, timeout=30)
        return out

    def status(self):
        out = self.call("status")
        assert out.returncode == 0, out.stderr
        return json.loads(out.stdout)


class StatusTests(unittest.TestCase):
    def test_list_names_the_methods(self):
        out = subprocess.run(["sh", str(PLUGIN), "list"], capture_output=True, text=True, check=True)
        self.assertEqual(sorted(json.loads(out.stdout)), ["enroll_start", "status"])

    def test_not_enrolled_by_default(self):
        s = Harness(self).status()
        self.assertFalse(s["enrolled"])
        self.assertEqual(s["peers"], [])

    def test_enrolled_state_prefix_and_name(self):
        h = Harness(self)
        (h.state / "device_token").write_text(SECRET + "\n")
        (h.state / "status").write_text("configuration renewed 2001:db8:1:20::/60\n")
        h.uci = {"hams_node.main.prefix60": "2001:db8:1:20::/60", "hams_node.main.ham_name": "w1aw.ham"}
        s = h.status()
        self.assertTrue(s["enrolled"])
        self.assertEqual(s["prefix60"], "2001:db8:1:20::/60")
        self.assertEqual(s["ham_name"], "w1aw.ham")
        self.assertEqual(s["state"], "configuration renewed 2001:db8:1:20::/60")

    def test_revoked_is_not_enrolled(self):
        h = Harness(self)
        (h.state / "status").write_text("revoked\n")
        s = h.status()
        self.assertFalse(s["enrolled"])
        self.assertEqual(s["state"], "revoked")

    def test_secrets_never_in_output(self):
        h = Harness(self)
        (h.state / "device_token").write_text(SECRET + "\n")
        (h.state / "wg.key").write_text("PRIVATEKEYVALUE\n")
        out = h.call("status")
        self.assertNotIn(SECRET, out.stdout)
        self.assertNotIn("PRIVATEKEYVALUE", out.stdout)

    def test_active_peers_from_ondemand_state(self):
        h = Harness(self)
        (h.ondemand / "peers").write_text("%s 200101 2001:db8:5::/60 [2001:db8::9]:51820\n" % KEY_A)
        peers = h.status()["peers"]
        self.assertEqual(peers, [{"prefix": "2001:db8:5::/60", "endpoint": "[2001:db8::9]:51820", "key": KEY_A}])

    def test_boundary_counters_and_integrity_indicator(self):
        h = Harness(self)
        h.counters = {"refused_encrypted": (7, 700), "refused_other": (3, 90)}
        b = h.status()["boundary"]
        self.assertTrue(b["active"])
        self.assertEqual(b["refused_encrypted"], 7)
        self.assertEqual(b["refused_other"], 3)
        self.assertTrue(b["integrity_only"])

    def test_boundary_missing_is_not_active(self):
        h = Harness(self)
        h.boundary = False
        b = h.status()["boundary"]
        self.assertFalse(b["active"])
        self.assertFalse(b["integrity_only"])
        self.assertEqual(b["refused_encrypted"], 0)


class EnrollTests(unittest.TestCase):
    def test_enroll_start_runs_the_enroll_command_and_status_shows_the_code(self):
        h = Harness(self)
        out = h.call("enroll_start")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("enroll enroll", h.calls.read_text())
        self.assertEqual(h.status()["code"], "ABCD-EFGH")

    def test_code_hidden_once_enrolled(self):
        h = Harness(self)
        h.enroll_out.write_text("On hams.com open x and enter the code: ABCD-EFGH\n")
        (h.state / "device_token").write_text(SECRET)
        self.assertEqual(h.status()["code"], "")


class AclTests(unittest.TestCase):
    def setUp(self):
        self.acl = json.loads(ACL.read_text())["luci-app-hams-node"]

    def test_read_grants(self):
        self.assertEqual(self.acl["read"]["ubus"]["hams-node"], ["status"])
        self.assertEqual(sorted(self.acl["read"]["uci"]), ["hams_node", "hams_node_ondemand"])

    def test_write_grants_only_this_app(self):
        self.assertEqual(self.acl["write"]["ubus"]["hams-node"], ["enroll_start"])
        self.assertEqual(sorted(self.acl["write"]["uci"]), ["hams_node", "hams_node_ondemand"])
        self.assertNotIn("file", self.acl["read"])
        self.assertNotIn("file", self.acl["write"])

    def test_menu_depends_on_the_acl(self):
        entry = json.loads(MENU.read_text())["admin/services/hams-node"]
        self.assertEqual(entry["depends"]["acl"], ["luci-app-hams-node"])
        self.assertEqual(entry["action"]["path"], "hams-node/status")

    def test_view_file_exists_for_the_menu_path(self):
        self.assertTrue((PKG / "htdocs" / "luci-static" / "resources" / "view" / "hams-node" / "status.js.in").is_file())


if __name__ == "__main__":
    unittest.main()
