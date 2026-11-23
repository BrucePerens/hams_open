# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-status with stand-ins for uclient-fetch (canned replies in order, requests logged), usign (a real Ed25519 check in
usign's file layout), wg, ip, ifdown/ifup, the init scripts and the report/traffic daemons (every call logged)."""

import base64
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import unittest

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-status"
FETCH = """#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
post = [a for a in args if a.startswith("--post-data=")][0]
open(os.environ["FAKE"] + "/calls", "a").write(args[-1] + " " + post + "\\n")
queue = os.environ["FAKE"] + "/replies"
if not os.path.exists(queue):
    sys.exit(1)
lines = open(queue).read().splitlines()
if not lines:
    sys.exit(1)
open(queue, "w").write("\\n".join(lines[1:]) + ("\\n" if lines[1:] else ""))
if lines[0] == "FAIL":
    sys.exit(1)
open(args[args.index("-O") + 1], "w").write(lines[0])
"""
# usign -V -m MESSAGE -p PUBFILE -x SIGFILE with usign's layout: line 2 of each file is base64("Ed" + keynum + key or signature)
USIGN = """#!/usr/bin/env python3
import base64, sys
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
a = sys.argv[1:]
msg = open(a[a.index("-m") + 1], "rb").read()
pub = base64.b64decode(open(a[a.index("-p") + 1]).read().splitlines()[1])
sig = base64.b64decode(open(a[a.index("-x") + 1]).read().splitlines()[1])
try:
    assert pub[:10] == sig[:10]
    Ed25519PublicKey.from_public_bytes(pub[10:]).verify(sig[10:], msg)
except (InvalidSignature, AssertionError):
    sys.exit(1)
"""
LOGGER = """#!/bin/sh
echo "$(basename "$0") $*" >> "$FAKE/actions"
"""
ENTRY_A = {"id": 3, "key": "KEYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "prefixes": ["2001:db8:ab00:30::/60"], "reason": "emergency"}
ENTRY_B = {"id": 4, "key": "KEYBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=", "prefixes": ["2001:db8:ab00:40::/60", "2001:db8:ab01::/48"], "reason": "abuse"}


class Signer:
    def __init__(self):
        self.key = Ed25519PrivateKey.generate()
        self.raw = self.key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        self.keynum = hashlib.sha256(self.raw).digest()[:8]
        self.public_line = base64.b64encode(b"Ed" + self.keynum + self.raw).decode()

    def reply(self, body):
        payload = json.dumps(body, separators=(",", ":"), sort_keys=True).encode()
        sig = base64.b64encode(b"Ed" + self.keynum + self.key.sign(payload)).decode()
        return json.dumps({"jsonrpc": "2.0", "id": None, "result": {"status": "success", "payload": base64.b64encode(payload).decode(), "signature": sig}})


class StatusTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.fake = self.root / "fake"
        self.fake.mkdir()
        self.state = self.root / "state"
        self.state.mkdir()
        self.signer = Signer()
        (self.state / "device_token").write_text("tok_en-1\n")
        (self.state / "wg.key").write_text("private\n")
        (self.state / "deprecation.pub").write_text(self.signer.public_line + "\n")
        self.cron = self.root / "crontab"
        bindir, initdir = self.root / "bin", self.root / "init.d"
        bindir.mkdir()
        initdir.mkdir()
        for name, text in (("uclient-fetch", FETCH), ("usign", USIGN), ("wg", LOGGER), ("ip", LOGGER), ("ifdown", LOGGER), ("ifup", LOGGER),
                           ("hams-node-report", LOGGER), ("hams-node-traffic", LOGGER), ("hams-node-mtls", LOGGER)):
            (bindir / name).write_text(text)
            (bindir / name).chmod(0o755)
        for name in ("hams-node-bird", "hams-node-ondemand"):
            (initdir / name).write_text(LOGGER.replace("basename \"$0\"", "echo init.d/%s" % name).replace('$(echo init.d/%s)' % name, "init.d/%s" % name))
            (initdir / name).chmod(0o755)
        self.initdir = initdir
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s" % (bindir, HERE, os.environ["PATH"]))

    def queue(self, *replies):
        (self.fake / "replies").write_text("\n".join(replies) + "\n")

    def status(self, state="ok", list_seq=1, reason="", signer=None):
        return (signer or self.signer).reply({"kind": "status", "state": state, "reason": reason, "list_seq": list_seq, "list_updated": "2026-11-23T00:00:00Z"})

    def run_script(self, command="run"):
        return subprocess.run(["sh", str(SCRIPT), "-u", "https://portal.example", "-s", str(self.state), "-c", str(self.cron), "-i", "wgtest", "-d", str(self.initdir), command],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def actions(self):
        path = self.fake / "actions"
        return path.read_text().splitlines() if path.exists() else []

    def calls(self):
        path = self.fake / "calls"
        return [(line.split(" ", 1)[0], json.loads(line.split(" --post-data=", 1)[1])["params"]) for line in path.read_text().splitlines()] if path.exists() else []

    def test_a_signed_disabled_turns_the_node_off_and_keeps_the_configuration(self):
        self.queue(self.status("disabled", reason="botnet traffic"))
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        actions = self.actions()
        for expected in ("ifdown wgtest", "ip -6 route flush dev wgtest", "init.d/hams-node-bird stop", "init.d/hams-node-ondemand stop", "hams-node-report stop", "hams-node-traffic stop"):
            self.assertIn(expected, actions)
        self.assertEqual((self.state / "disabled").read_text().strip(), "botnet traffic", "the reason is kept for LuCI")
        self.assertTrue((self.state / "device_token").exists(), "configuration kept")
        self.assertTrue((self.state / "wg.key").exists(), "configuration kept")
        self.assertEqual(self.calls()[0][0], "https://portal.example/api/ipv6/status")
        self.assertEqual(self.calls()[0][1], {"token": "tok_en-1"})
        self.queue(self.status("disabled", reason="botnet traffic"))
        before = len(self.actions())
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(len(self.actions()), before, "already off: nothing is repeated")

    def test_no_information_never_disables_a_router_fail_open(self):
        for replies in (["FAIL"], [], ["not json"], ['{"result":{"status":"error","message":"refused"}}'],
                        [Signer().reply({"kind": "status", "state": "disabled", "reason": "x", "list_seq": 1})]):
            if replies:
                self.queue(*replies)
            else:
                (self.fake / "replies").unlink(missing_ok=True)
            result = self.run_script()
            self.assertEqual(result.returncode, 4, (replies, result.stderr))
            self.assertEqual(self.actions(), [], "unreachable, unsigned, refused or wrongly signed: the router stays as it is")
            self.assertFalse((self.state / "disabled").exists())
        reply = json.loads(self.status("disabled", reason="x"))
        reply["result"]["payload"] = base64.b64encode(base64.b64decode(reply["result"]["payload"]).replace(b"disabled", b"disablex")).decode()
        self.queue(json.dumps(reply))
        self.assertEqual(self.run_script().returncode, 4, "a tampered payload")
        self.assertEqual(self.actions(), [])
        (self.state / "deprecation.pub").unlink()
        self.queue(self.status("disabled", reason="x"))
        self.assertEqual(self.run_script().returncode, 4, "no key to verify with: no information")
        self.assertEqual(self.actions(), [])

    def test_an_unreachable_server_keeps_a_disabled_router_disabled(self):
        self.queue(self.status("disabled", reason="r"))
        self.run_script()
        self.queue("FAIL")
        self.assertEqual(self.run_script().returncode, 4)
        self.assertTrue((self.state / "disabled").exists(), "the last known state is kept")
        self.assertNotIn("ifup wgtest", self.actions())

    def test_a_signed_ok_turns_it_back_on_and_an_older_status_is_a_replay(self):
        self.queue(self.status("disabled", reason="r", list_seq=5))
        self.run_script()
        self.queue(self.status("ok", list_seq=4))
        self.assertEqual(self.run_script().returncode, 4, "older than the last accepted status: replay, ignored")
        self.assertTrue((self.state / "disabled").exists())
        self.queue(self.status("ok", list_seq=5), self.signer.reply({"kind": "full", "seq": 5, "entries": []}))
        self.assertEqual(self.run_script().returncode, 0)
        self.assertFalse((self.state / "disabled").exists())
        for expected in ("ifup wgtest", "init.d/hams-node-bird start", "init.d/hams-node-ondemand start", "hams-node-report start", "hams-node-traffic start"):
            self.assertIn(expected, self.actions())

    def full(self, seq, *entries):
        return self.signer.reply({"kind": "full", "seq": seq, "issued": "1999-01-01T00:00:00Z", "entries": list(entries)})

    def delta(self, old, seq, add=(), remove=()):
        return self.signer.reply({"kind": "delta", "from": old, "seq": seq, "add": list(add), "remove": list(remove)})

    def listed(self):
        return sorted((self.state / "deprecation_list").read_text().split("\n")[:-1]) if (self.state / "deprecation_list").exists() else []

    def test_deprecation_full_then_delta_with_removal_and_local_enforcement(self):
        self.queue(self.status(list_seq=2), self.full(2, ENTRY_A))
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.calls()[1], ("https://portal.example/api/ipv6/deprecations", {"token": "tok_en-1", "have_seq": 0, "have_time": self.calls()[1][1]["have_time"]}))
        self.assertIn("wg set wgtest peer %s remove" % ENTRY_A["key"], self.actions(), "the peer is evicted")
        self.assertIn("ip -6 route replace unreachable 2001:db8:ab00:30::/60", self.actions(), "its prefix is dropped")
        self.assertEqual((self.state / "deprecation_seq").read_text().strip(), "2")
        self.queue(self.status(list_seq=3), self.delta(2, 3, add=[ENTRY_B], remove=[ENTRY_A["key"]]))
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.calls()[3][1]["have_seq"], 2, "only the delta since the held sequence number is asked for")
        self.assertEqual(self.listed(), ["%s 2001:db8:ab00:40::/60,2001:db8:ab01::/48" % ENTRY_B["key"]], "removal applied, addition kept")
        self.assertIn("ip -6 route del unreachable 2001:db8:ab00:30::/60", self.actions(), "a removal lifts the block")
        self.assertIn("ip -6 route replace unreachable 2001:db8:ab01::/48", self.actions())

    def test_revoked_member_certificates_are_kept_for_the_mtls_check_and_never_touch_wireguard(self):
        cert = {"type": "revoked_member_cert", "key": "abcdef", "issuer_sha256": "a1" * 32, "revoked_at": "2026-11-20T12:00:00Z", "reason": "key_compromise"}
        self.queue(self.status(list_seq=2), self.full(2, ENTRY_A, cert))
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.listed(), sorted(["%s 2001:db8:ab00:30::/60" % ENTRY_A["key"], "cert:%s:abcdef " % ("a1" * 32)]))
        self.assertFalse([a for a in self.actions() if "abcdef" in a], "a certificate serial is never given to wg or ip")
        self.assertNotIn("hams-node-mtls -s %s apply" % self.state, self.actions(), "management is not under mtls here")
        (self.state / "mtls").mkdir()
        (self.state / "mtls" / "nginx.conf").write_text("x")
        self.queue(self.status(list_seq=3), self.delta(2, 3, remove=["abcdef"]))
        self.assertEqual(self.run_script().returncode, 0)
        self.assertIn("hams-node-mtls -s %s apply" % self.state, self.actions(), "the front end is told to re-read the list")
        self.assertEqual(self.listed(), ["%s 2001:db8:ab00:30::/60" % ENTRY_A["key"]], "a delta removal lifts the certificate entry")

    def test_a_gap_a_replay_and_a_bad_signature_are_refused_and_a_gap_asks_for_the_full_list(self):
        self.queue(self.status(list_seq=2), self.full(2, ENTRY_A))
        self.run_script()
        self.queue(self.status(list_seq=9), self.delta(5, 9, remove=[ENTRY_A["key"]]), self.full(9, ENTRY_B))
        self.assertEqual(self.run_script().returncode, 0)
        asked = [c[1]["have_seq"] for c in self.calls() if c[0].endswith("deprecations")]
        self.assertEqual(asked, [0, 2, 0], "a delta that does not line up makes the router ask for the full list")
        self.assertEqual(self.listed(), ["%s 2001:db8:ab00:40::/60,2001:db8:ab01::/48" % ENTRY_B["key"]])
        before = self.listed()
        self.queue(self.status(list_seq=10), self.delta(2, 10, remove=[ENTRY_B["key"]]), self.delta(2, 10, remove=[ENTRY_B["key"]]))
        self.assertEqual(self.run_script().returncode, 4, "a replayed old delta never matches the held number, and the full-list retry is a delta too: refused")
        self.assertEqual(self.listed(), before)
        forged = Signer().reply({"kind": "full", "seq": 11, "entries": []})
        self.queue(self.status(list_seq=11), forged)
        self.assertEqual(self.run_script().returncode, 4, "a wrongly signed list is refused")
        self.assertEqual(self.listed(), before)

    def test_a_disabled_router_does_not_fetch_the_list(self):
        self.queue(self.status("disabled", reason="r", list_seq=7))
        self.run_script()
        self.assertEqual([c[0] for c in self.calls()], ["https://portal.example/api/ipv6/status"])

    def test_start_and_stop_manage_one_cron_line_every_two_minutes(self):
        self.assertEqual(self.run_script("start").returncode, 0)
        self.assertEqual(self.run_script("start").returncode, 0)
        self.assertEqual(self.cron.read_text().splitlines(), ["*/2 * * * * /usr/sbin/hams-node-status run"])
        self.assertEqual(self.run_script("stop").returncode, 0)
        self.assertEqual(self.cron.read_text(), "")


if __name__ == "__main__":
    unittest.main()
