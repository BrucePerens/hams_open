# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-enroll with stand-ins for uclient-fetch (replies from files), wg and uci."""

import json
import os
import pathlib
import re
import stat
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
PACKAGE = HERE.parent
SCRIPT = PACKAGE / "files" / "usr" / "sbin" / "hams-node-enroll"
KEY = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
FETCH = """#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
url = args[-1]
post = [a for a in args if a.startswith("--post-data=")]
with open(os.environ["FAKE"] + "/calls", "a") as log:
    log.write(url + " " + (post[0] if post else "") + "\\n")
queue = os.environ["FAKE"] + "/" + url.rsplit("/", 1)[1]
if not os.path.exists(queue):
    sys.exit(1)
lines = open(queue).read().splitlines()
if not lines:
    sys.exit(1)
open(queue, "w").write("\\n".join(lines[1:]))
open(args[args.index("-O") + 1], "w").write(lines[0])
"""


def reply(**result):
    return json.dumps({"jsonrpc": "2.0", "id": None, "result": result})


CONFIG = dict(prefix60="2001:db8:ab00:10::/60", asn=4200000007, ham_prefix="2001:db8:ab00::/40", server_key=KEY, callsign="N0CALL")


class EnrollTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.fake = self.root / "fake"
        self.fake.mkdir()
        self.state = self.root / "state"
        self.cron = self.root / "crontab"
        bindir = self.root / "bin"
        bindir.mkdir()
        for name, body in {
            "uclient-fetch": FETCH,
            "wg": '#!/bin/sh\ncase $1 in genkey) echo "%s";; pubkey) echo "%s";; esac\n' % (KEY, KEY),
            "uci": '#!/bin/sh\ncase $* in *"get hams_node_enroll.main.url"*) cat "$FAKE/uci_url" 2>/dev/null; exit 0 ;; esac\necho "$*" >> "$FAKE/uci"\n',
        }.items():
            (bindir / name).write_text(body)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s" % (bindir, HERE, os.environ["PATH"]))

    def queue(self, endpoint, *replies):
        (self.fake / endpoint).write_text("\n".join(replies))

    def run_script(self, *args):
        return subprocess.run(["sh", str(SCRIPT), "-u", "https://portal.example", "-s", str(self.state), "-c", str(self.cron), "-i", "0", *args],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def run_uci_url(self, url, *args):
        """Run without -u, so the portal URL comes from uci (or the built-in default when url is None)."""
        if url is not None:
            (self.fake / "uci_url").write_text(url)
        return subprocess.run(["sh", str(SCRIPT), "-s", str(self.state), "-c", str(self.cron), "-i", "0", *args],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def calls(self):
        path = self.fake / "calls"
        return path.read_text().splitlines() if path.exists() else []

    def enroll(self):
        self.queue("start", reply(status="success", device_code="devcode123", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="pending", message="authorization_pending"), reply(status="success", token="tok_en-1", **CONFIG))
        return self.run_script("enroll")

    def test_enroll_shows_the_code_stores_the_token_root_only_and_writes_the_config(self):
        result = self.enroll()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ABCD-EFGH", result.stdout)
        token = self.state / "device_token"
        self.assertEqual(token.read_text().strip(), "tok_en-1")
        for secret in (token, self.state / "wg.key"):
            self.assertEqual(stat.S_IMODE(secret.stat().st_mode), 0o600, secret)
        uci = (self.fake / "uci").read_text()
        self.assertIn("hams_node_ondemand.main.ham_prefix=2001:db8:ab00::/40", uci)
        self.assertIn("hams_node_ondemand.main.lookup_url=https://portal.example/api/ipv6/directory/lookup", uci)
        self.assertIn("hams_node.main.prefix60=2001:db8:ab00:10::/60", uci)
        self.assertIn("hams-node-enroll fetch", self.cron.read_text())
        polls = [c for c in self.calls() if c.endswith("/poll " + c.split(" ", 1)[1])]
        self.assertIn('"device_code":"devcode123","public_key":"%s"' % KEY, polls[0])

    def test_one_time_token_is_sent_as_the_poll_secret_without_asking_for_a_code(self):
        self.queue("poll", reply(status="success", token="tok_en-2", **CONFIG))
        result = self.run_script("enroll", "oneTimeToken_abc")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls()), 1)
        self.assertIn('"device_code":"oneTimeToken_abc"', self.calls()[0])

    def test_refused_or_expired_enrolment_stops_without_a_token(self):
        self.queue("start", reply(status="success", device_code="d1", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="error", message="refused"), reply(status="success", token="never", **CONFIG))
        result = self.run_script("enroll")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len([c for c in self.calls() if "/poll" in c]), 1, "no retry loop")
        self.assertFalse((self.state / "device_token").exists())

    def test_fetch_renews_and_a_revoked_token_stops_for_good(self):
        self.assertEqual(self.enroll().returncode, 0)
        (self.fake / "uci").unlink()
        self.queue("config", reply(status="success", **dict(CONFIG, prefix60="2001:db8:ab00:20::/60")), reply(status="error", message="refused"), reply(status="success", **CONFIG))
        renewed = self.run_script("fetch")
        self.assertEqual(renewed.returncode, 0, renewed.stderr)
        self.assertIn("hams_node.main.prefix60=2001:db8:ab00:20::/60", (self.fake / "uci").read_text())
        self.assertIn('"token":"tok_en-1"', [c for c in self.calls() if "/config" in c][0])
        revoked = self.run_script("fetch")
        self.assertEqual(revoked.returncode, 3)
        self.assertFalse((self.state / "device_token").exists())
        self.assertNotIn("hams-node-enroll fetch", self.cron.read_text())
        before = len(self.calls())
        again = self.run_script("fetch")
        self.assertEqual(again.returncode, 3)
        self.assertEqual(len(self.calls()), before, "a revoked node does not ask again")

    def test_unreachable_portal_keeps_the_token_for_the_next_run(self):
        self.assertEqual(self.enroll().returncode, 0)
        result = self.run_script("fetch")  # no queued reply: uclient-fetch fails
        self.assertEqual(result.returncode, 4)
        self.assertTrue((self.state / "device_token").exists())

    def test_no_password_anywhere_in_the_package(self):
        for path in sorted((PACKAGE / "files").rglob("*")) + [PACKAGE / "Makefile"]:
            if path.is_file():
                self.assertIsNone(re.search(r"pass(word|wd)|read -s", path.read_text(), re.I), path)
        self.assertIsNone(re.search(r"pass(word|wd)", (PACKAGE / "files/etc/config/hams_node_enroll").read_text(), re.I))

    def test_portal_url_comes_from_uci_when_no_u_option(self):
        self.queue("start", reply(status="success", device_code="d1", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="success", token="tok_uci", **CONFIG))
        result = self.run_uci_url("https://uci.example.test", "enroll")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all(c.startswith("https://uci.example.test/api/ipv6/enroll/") for c in self.calls()), self.calls())
        self.assertIn("lookup_url=https://uci.example.test/api/ipv6/directory/lookup", (self.fake / "uci").read_text())

    def test_built_in_default_is_used_only_when_uci_url_is_unset(self):
        self.queue("start", reply(status="success", device_code="d1", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="pending", message="authorization_pending"))
        result = self.run_uci_url(None, "enroll")
        self.assertEqual(result.returncode, 4, "no queued poll: the built-in portal is unreachable here")
        self.assertTrue(self.calls()[0].startswith("https://hams.com/api/ipv6/enroll/start"), self.calls())

    def test_plain_http_is_refused_unless_the_host_is_loopback(self):
        for url in ("http://portal.example.test", "http://10.0.2.2:8097", "http://127.0.0.1.evil.example", "ftp://127.0.0.1"):  # burn-ignore-ssrf-test-value: URL-scheme fixtures only, nothing connects
            result = self.run_uci_url(url, "enroll", "tok")
            self.assertEqual(result.returncode, 1, url)
            self.assertIn("https://", result.stderr, url)
        self.assertEqual(self.calls(), [], "a refused URL never reaches uclient-fetch")
        for url in ("http://127.0.0.1:8097", "http://localhost", "http://localhost:8097/"):  # burn-ignore-ssrf-test-value: URL-scheme fixtures only, nothing connects
            self.queue("poll", reply(status="success", token="tok_local", **CONFIG))
            result = self.run_uci_url(url, "enroll", "tok")
            self.assertEqual(result.returncode, 0, (url, result.stderr))

    def test_bad_token_characters_never_reach_a_request(self):
        result = self.run_script("enroll", 'x","evil":"1')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
