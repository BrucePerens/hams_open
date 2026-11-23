# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-key rotate: stand-ins for wg (new key per call), uclient-fetch, uci and hams-node-endpoint."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-key"
OLD = "OLDOLDOLDOLDOLDOLDOLDOLDOLDOLDOLDOLDOLDOLDO="
NEW = "NEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNE="
NEW_PUB = "PUBPUBPUBPUBPUBPUBPUBPUBPUBPUBPUBPUBPUBPUB="
WG = """#!/bin/sh
echo "wg $*" >> "$FAKE/calls"
case $1 in
genkey) echo "%s" ;;
pubkey) cat >/dev/null; echo "%s" ;;
esac
""" % (NEW, NEW_PUB)
FETCH = """#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
post = [a for a in args if a.startswith("--post-data=")]
with open(os.environ["FAKE"] + "/calls", "a") as log:
    log.write("fetch " + args[-1] + " " + (post[0] if post else "") + "\\n")
reply = os.environ["FAKE"] + "/reply"
if not os.path.exists(reply):
    sys.exit(1)
open(args[args.index("-O") + 1], "w").write(open(reply).read())
"""
UCI = '#!/bin/sh\n[ "$1" = -q ] && shift\n[ "$1" = get ] && cat "$FAKE/uci/$2" 2>/dev/null\nexit 0\n'


def reply(**result):
    return json.dumps({"jsonrpc": "2.0", "id": None, "result": result})


class KeyTests(unittest.TestCase):
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
        for name, body in {"wg": WG, "uclient-fetch": FETCH, "uci": UCI, "hams-node-endpoint": '#!/bin/sh\necho "endpoint $*" >> "$FAKE/calls"\n'}.items():
            (bindir / name).write_text(body)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s" % (bindir, HERE, os.environ["PATH"]))
        (self.state / "wg.key").write_text(OLD + "\n")
        (self.state / "wg.key").chmod(0o600)
        (self.state / "device_token").write_text("tok123\n")
        (self.state / "endpoint_sent").write_text("192.0.2.7:51820 1\n")
        (self.fake / "reply").write_text(reply(status="success"))

    def rotate(self):
        return subprocess.run(["sh", str(SCRIPT), "-u", "https://portal.example", "-s", str(self.state), "rotate"],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def calls(self):
        path = self.fake / "calls"
        return path.read_text().splitlines() if path.exists() else []

    def test_rotation_sends_only_the_public_key_and_keeps_the_new_private_key(self):
        result = self.rotate()
        self.assertEqual(result.returncode, 0, result.stderr)
        sent = [c for c in self.calls() if c.startswith("fetch ")]
        self.assertEqual(len(sent), 1)
        self.assertIn("/api/ipv6/enroll/rekey", sent[0])
        body = json.loads(sent[0].split("--post-data=")[1])["params"]
        self.assertEqual(body, {"token": "tok123", "public_key": NEW_PUB})
        everything = "\n".join(self.calls()) + result.stdout + result.stderr
        self.assertNotIn(NEW, everything)
        self.assertNotIn(OLD, everything)
        self.assertEqual((self.state / "wg.key").read_text().strip(), NEW)
        self.assertEqual((self.state / "wg.key").stat().st_mode & 0o777, 0o600)
        self.assertFalse((self.state / "wg.key.new").exists())

    def test_a_running_tunnel_gets_the_new_key_and_the_endpoint_is_republished(self):
        self.assertEqual(self.rotate().returncode, 0)
        calls = self.calls()
        self.assertIn("wg set wg-hams private-key %s" % (self.state / "wg.key"), calls)
        self.assertFalse((self.state / "endpoint_sent").exists())
        self.assertIn("endpoint update", calls)

    def test_a_refused_or_unreachable_rotation_keeps_the_old_key(self):
        (self.fake / "reply").write_text(reply(status="error", message="refused"))
        self.assertEqual(self.rotate().returncode, 1)
        (self.fake / "reply").unlink()
        self.assertEqual(self.rotate().returncode, 4)
        self.assertEqual((self.state / "wg.key").read_text().strip(), OLD)
        self.assertFalse((self.state / "wg.key.new").exists())
        self.assertFalse(any(c.startswith("wg set") for c in self.calls()))
        self.assertTrue((self.state / "endpoint_sent").exists())

    def test_not_enrolled_changes_nothing(self):
        (self.state / "device_token").unlink()
        self.assertEqual(self.rotate().returncode, 1)
        self.assertEqual((self.state / "wg.key").read_text().strip(), OLD)
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
