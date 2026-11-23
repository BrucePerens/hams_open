# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-endpoint: real openssl, stand-ins for uclient-fetch, ubus and uci; the MAC is checked with the portal's own formula
(ham_ipv6net/models/directory.py update_mac) written out with the cryptography library."""

import base64
import hashlib
import hmac
import json
import os
import pathlib
import subprocess
import tempfile
import time
import unittest
from urllib.parse import urlparse

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey, X25519PublicKey

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-endpoint"
HOTPLUG = HERE.parent / "files" / "etc" / "hotplug.d" / "iface" / "90-hams-node-endpoint"
RAW = serialization.Encoding.Raw


def b64(raw):
    return base64.b64encode(raw).decode()


def portal_mac(server_private, public_key, endpoint, timestamp):
    shared = server_private.exchange(X25519PublicKey.from_public_bytes(base64.b64decode(public_key)))
    return hmac.new(shared, ("%s\n%s\n%d" % (public_key, endpoint, timestamp)).encode(), hashlib.sha256).hexdigest()


FETCH = """#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
post = [a for a in args if a.startswith("--post-data=")]
with open(os.environ["FAKE"] + "/calls", "a") as log:
    log.write(args[-1] + " " + (post[0] if post else "") + "\\n")
reply = os.environ["FAKE"] + "/reply"
if not os.path.exists(reply):
    sys.exit(1)
open(args[args.index("-O") + 1], "w").write(open(reply).read())
"""
UBUS = """#!/bin/sh
case "$*" in
*network.interface.wan*) cat "$FAKE/wan_status" ;;
esac
"""
UCI = """#!/bin/sh
[ "$1" = -q ] && shift
[ "$1" = get ] && cat "$FAKE/uci/$2" 2>/dev/null
"""


class EndpointTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.fake = self.root / "fake"
        (self.fake / "uci").mkdir(parents=True)
        self.state = self.root / "state"
        self.state.mkdir()
        bindir = self.root / "bin"
        bindir.mkdir()
        for name, body in {"uclient-fetch": FETCH, "ubus": UBUS, "uci": UCI}.items():
            (bindir / name).write_text(body)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s" % (bindir, HERE, os.environ["PATH"]))
        self.device = X25519PrivateKey.generate()
        self.server = X25519PrivateKey.generate()
        self.public = b64(self.device.public_key().public_bytes(RAW, serialization.PublicFormat.Raw))
        self.private = b64(self.device.private_bytes(RAW, serialization.PrivateFormat.Raw, serialization.NoEncryption()))
        (self.state / "wg.key").write_text(self.private + "\n")
        (self.state / "wg.key").chmod(0o600)
        self.set_uci("hams_node.main.server_key", b64(self.server.public_key().public_bytes(RAW, serialization.PublicFormat.Raw)))
        self.wan("192.0.2.7")
        (self.fake / "reply").write_text(json.dumps({"jsonrpc": "2.0", "id": None, "result": {"status": "success"}}))

    def set_uci(self, name, value):
        (self.fake / "uci" / name).write_text(value + "\n")

    def wan(self, address):
        (self.fake / "wan_status").write_text(json.dumps({"up": True, "ipv4-address": [{"address": address, "mask": 24}]}))

    def run_script(self, *args):
        return subprocess.run(["sh", str(SCRIPT), "-u", "https://portal.example", "-s", str(self.state), *args],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def posts(self):
        path = self.fake / "calls"
        out = []
        for line in path.read_text().splitlines() if path.exists() else []:
            url, _, data = line.partition(" --post-data=")
            out.append((url, json.loads(data)["params"]))
        return out

    def test_sends_a_signed_update_the_portal_accepts(self):
        result = self.run_script("update")
        self.assertEqual(result.returncode, 0, result.stderr)
        (url, params), = self.posts()
        self.assertEqual(url, "https://portal.example/api/ipv6/directory/endpoint")
        self.assertEqual(params["public_key"], self.public)
        self.assertEqual(params["endpoint"], "192.0.2.7:51820")
        self.assertLess(abs(params["timestamp"] - time.time()), 30)
        self.assertEqual(params["mac"], portal_mac(self.server, self.public, params["endpoint"], params["timestamp"]))

    def test_the_private_key_is_in_no_request_log_or_output(self):
        result = self.run_script("update")
        sent = (self.fake / "calls").read_text()
        self.assertNotIn(self.private, sent + result.stdout + result.stderr)
        self.assertNotIn(self.private.rstrip("="), sent + result.stdout + result.stderr)

    def test_unchanged_address_sends_nothing_more_but_a_change_sends_at_once(self):
        self.run_script("update")
        self.run_script("update")
        self.assertEqual(len(self.posts()), 1)
        self.wan("198.51.100.9")
        self.run_script("update")
        self.assertEqual([p["endpoint"] for _, p in self.posts()], ["192.0.2.7:51820", "198.51.100.9:51820"])

    def test_a_stale_record_is_refreshed_even_when_nothing_changed(self):
        self.run_script("update")
        sent = self.state / "endpoint_sent"
        sent.write_text("192.0.2.7:51820 %d\n" % (time.time() - 7 * 3600))
        self.run_script("update")
        self.assertEqual(len(self.posts()), 2)

    def test_a_refused_or_unreachable_update_is_retried_next_time(self):
        (self.fake / "reply").write_text(json.dumps({"jsonrpc": "2.0", "id": None, "result": {"status": "error", "message": "refused"}}))
        self.assertEqual(self.run_script("update").returncode, 1)
        (self.fake / "reply").unlink()
        self.assertEqual(self.run_script("update").returncode, 4)
        (self.fake / "reply").write_text(json.dumps({"jsonrpc": "2.0", "id": None, "result": {"status": "success"}}))
        self.assertEqual(self.run_script("update").returncode, 0)
        self.assertEqual(len(self.posts()), 3)

    def test_configured_port_and_host_are_used(self):
        self.set_uci("hams_node_endpoint.main.port", "51999")
        self.set_uci("hams_node_endpoint.main.host", "gw.example.org")
        self.run_script("update")
        self.assertEqual(self.posts()[0][1]["endpoint"], "gw.example.org:51999")

    def test_nothing_is_sent_without_a_key_a_server_key_or_a_wan_address(self):
        (self.fake / "uci" / "hams_node.main.server_key").unlink()
        self.assertEqual(self.run_script("update").returncode, 1)
        self.set_uci("hams_node.main.server_key", b64(self.server.public_key().public_bytes(RAW, serialization.PublicFormat.Raw)))
        (self.fake / "wan_status").write_text("{}")
        self.assertEqual(self.run_script("update").returncode, 1)
        self.wan("192.0.2.7")
        (self.state / "wg.key").unlink()
        self.assertEqual(self.run_script("update").returncode, 1)
        self.assertEqual(self.posts(), [])

    def test_start_and_stop_manage_one_cron_line(self):
        cron = self.root / "crontab"
        self.assertEqual(self.run_script("-c", str(cron), "start").returncode, 0)
        self.run_script("-c", str(cron), "start")
        self.assertEqual(sum("hams-node-endpoint" in l for l in cron.read_text().splitlines()), 1)
        self.run_script("-c", str(cron), "stop")
        self.assertNotIn("hams-node-endpoint", cron.read_text())

    def test_hotplug_runs_an_update_on_wan_ifup_only(self):
        for action, interface, expect in (("ifup", "wan", 1), ("ifupdate", "wan", 1), ("ifdown", "wan", 0), ("ifup", "lan", 0)):
            log = self.root / "hp.log"
            log.unlink(missing_ok=True)
            hp = self.root / "bin" / "hams-node-endpoint"
            hp.write_text('#!/bin/sh\necho "$*" >> "%s"\n' % log)
            hp.chmod(0o755)
            env = dict(self.env, ACTION=action, INTERFACE=interface)
            subprocess.run(["sh", str(HOTPLUG)], env=env, capture_output=True, text=True, timeout=30)
            time.sleep(0.3)
            self.assertEqual(len(log.read_text().splitlines()) if log.exists() else 0, expect, (action, interface))


if __name__ == "__main__":
    unittest.main()
