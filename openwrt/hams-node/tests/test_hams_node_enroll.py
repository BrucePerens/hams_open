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

from test_hams_node_status import USIGN, Signer

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
reply = lines[0]
if reply.startswith("SIGN "):
    # a reply the portal would sign for THIS request: "@request" in the body is the request's own nonce
    import base64, hashlib, json
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    spec = json.loads(reply[5:])
    body = dict(spec["body"])
    if body.get("nonce") == "@request":
        body["nonce"] = json.loads([a for a in args if a.startswith("--post-data=")][0].split("=", 1)[1])["params"].get("nonce", "none")
    key = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(spec["seed"]))
    raw = key.public_key().public_bytes_raw()
    payload = json.dumps(body, separators=(",", ":"), sort_keys=True).encode()
    sig = base64.b64encode(b"Ed" + hashlib.sha256(raw).digest()[:8] + key.sign(payload)).decode()
    result = dict(spec["plain"], signed={"payload": base64.b64encode(payload).decode(), "signature": sig})
    reply = json.dumps({"jsonrpc": "2.0", "id": None, "result": result})
open(args[args.index("-O") + 1], "w").write(reply)
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
            "usign": USIGN,
            "wg": '#!/bin/sh\ncase $1 in genkey) echo "%s";; pubkey) echo "%s";; show) echo "peerA"; echo "peerB";; *) echo "wg $*" >> "$FAKE/wg";; esac\n' % (KEY, KEY),
            "ip": '#!/bin/sh\necho "ip $*" >> "$FAKE/ip"\n',
            "uci": '#!/bin/sh\ncase $* in *"get hams_node_enroll.main.url"*) cat "$FAKE/uci_url" 2>/dev/null; exit 0 ;; esac\necho "$*" >> "$FAKE/uci"\n',
        }.items():
            (bindir / name).write_text(body)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s:%s" % (bindir, HERE, HERE.parent / "files" / "usr" / "sbin", os.environ["PATH"]),
                        HAMS_NODE_PINNED_KEYS=str(self.root / "pinned_keys"))
        self.signer = Signer()

    def pin(self, *signers):
        (self.root / "pinned_keys").write_text("# the package list\n" + "".join(s.public_line + "\n" for s in signers))

    def signed(self, signer=None, kind="config", nonce="@request", **override):
        """A queued config reply signed for the request; the flat fields are what an attacker could have changed."""
        config = dict(CONFIG, **override)
        body = dict(config, kind=kind, nonce=nonce, issued=1)
        return "SIGN " + json.dumps({"plain": dict(config, status="success", prefix60="2001:db8:dead::/60"), "body": body, "seed": (signer or self.signer).seed})

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

    def test_enrol_records_the_owner_callsign_and_unenroll_removes_it(self):
        self.assertEqual(self.enroll().returncode, 0)
        self.assertEqual((self.state / "owner").read_text().strip(), "N0CALL")
        self.queue("unenroll", reply(status="success"))
        self.assertEqual(self.run_script("unenroll").returncode, 0)
        self.assertFalse((self.state / "owner").exists())

    def test_the_deprecation_key_from_the_config_is_kept_for_hams_node_status_and_removed_on_unenroll(self):
        self.queue("start", reply(status="success", device_code="devcode123", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="success", token="tok_en-1", deprecation_key="RWSignKeyLine+/0=", **CONFIG))
        self.assertEqual(self.run_script("enroll").returncode, 0)
        self.assertEqual((self.state / "deprecation.pub").read_text().strip(), "RWSignKeyLine+/0=")
        self.queue("unenroll", reply(status="success"))
        self.assertEqual(self.run_script("unenroll").returncode, 0)
        self.assertFalse((self.state / "deprecation.pub").exists())

    def test_the_owners_ssh_keys_from_the_config_are_kept_and_removed_on_unenroll(self):
        keys = [{"serial": "1a2b", "key": "ecdsa-sha2-nistp256 AAAAE2VjZHNh"}, {"serial": "bad;rm", "key": "x"}, {"serial": "3c", "key": "ecdsa-sha2-nistp256 BBBB"}]
        self.queue("start", reply(status="success", device_code="devcode123", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="success", token="tok_en-1", ssh_keys=keys, **CONFIG))
        self.assertEqual(self.run_script("enroll").returncode, 0)
        self.assertEqual((self.state / "ssh_keys").read_text().splitlines(), ["1a2b ecdsa-sha2-nistp256 AAAAE2VjZHNh", "3c ecdsa-sha2-nistp256 BBBB"])
        self.queue("unenroll", reply(status="success"))
        self.assertEqual(self.run_script("unenroll").returncode, 0)
        self.assertFalse((self.state / "ssh_keys").exists())

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

    def test_an_answer_that_is_not_the_explicit_refusal_keeps_the_token_and_the_cron_line(self):
        self.assertEqual(self.enroll().returncode, 0)
        for answer in (reply(status="error", message="rate_limited"), reply(status="maintenance"), reply(status="error"), "<html>502 Bad Gateway</html>"):
            self.queue("config", answer)
            result = self.run_script("fetch")
            self.assertEqual(result.returncode, 4, (answer, result.stderr))
            self.assertTrue((self.state / "device_token").exists(), answer)
            self.assertIn("hams-node-enroll fetch", self.cron.read_text(), answer)
            self.assertNotEqual((self.state / "status").read_text().strip(), "revoked", answer)

    def test_unenroll_revokes_server_side_then_removes_peers_key_uci_and_cron(self):
        self.assertEqual(self.enroll().returncode, 0)
        (self.fake / "uci").unlink()
        self.queue("unenroll", reply(status="success"))
        result = self.run_script("unenroll")
        self.assertEqual(result.returncode, 0, result.stderr)
        sent = [c for c in self.calls() if "/unenroll" in c]
        self.assertIn('"token":"tok_en-1"', sent[0])
        for name in ("device_token", "wg.key", "status"):
            self.assertFalse((self.state / name).exists(), name)
        self.assertNotIn("hams-node-enroll fetch", self.cron.read_text())
        wg = (self.fake / "wg").read_text()
        self.assertIn("wg set wg-hams peer peerA remove", wg)
        self.assertIn("wg set wg-hams peer peerB remove", wg)
        self.assertIn("ip link del wg-hams", (self.fake / "ip").read_text())
        uci = (self.fake / "uci").read_text()
        for section in ("hams_node.main", "hams_node_ondemand.main"):
            self.assertIn("delete " + section, uci)

    def test_unenroll_with_an_unreachable_portal_keeps_everything_for_a_retry(self):
        self.assertEqual(self.enroll().returncode, 0)
        result = self.run_script("unenroll")  # no queued reply: uclient-fetch fails
        self.assertEqual(result.returncode, 4)
        self.assertTrue((self.state / "device_token").exists())
        self.assertTrue((self.state / "wg.key").exists())

    def test_unenroll_after_a_revocation_still_cleans_up_locally(self):
        self.assertEqual(self.enroll().returncode, 0)
        self.queue("config", reply(status="error", message="refused"))
        self.assertEqual(self.run_script("fetch").returncode, 3)
        result = self.run_script("unenroll")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.state / "wg.key").exists())
        self.assertNotIn("/unenroll", " ".join(self.calls()), "no token left to send")

    def test_no_password_anywhere_in_the_package(self):
        # every package file is scanned except hams-node-bird (BGP "password", a BIRD keyword) hams-node-ssh (turns dropbear password login off) and hams-node-mtls with its sso handler (turns the router password login off)
        for path in sorted((PACKAGE / "files").rglob("*")) + [PACKAGE / "Makefile"] + sorted((PACKAGE / "pkg").glob("*.mk")):
            if path.is_file() and "bird" not in path.name and "ssh" not in path.name and "mtls" not in path.name and "/sso/" not in str(path):
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

    def test_a_reply_with_characters_outside_its_fields_is_refused_before_any_setting_is_written(self):
        for field, value in (("ham_prefix", "2001:db8::/40; } flush ruleset"), ("prefix60", "2001:db8:ab00:10::/60 x"), ("asn", "42; reboot"),
                             ("server_key", KEY + "\n$(id)"), ("callsign", "N0CALL\nroot")):
            with self.subTest(field):
                self.setUp()
                self.queue("poll", reply(status="success", token="tok_en-1", **dict(CONFIG, **{field: value})))
                result = self.run_script("enroll", "tok")
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("refusing the configuration", result.stderr)
                self.assertFalse((self.fake / "uci").exists(), "nothing reaches uci")

    def enroll_pinned(self, config_reply):
        self.pin(self.signer)
        self.queue("start", reply(status="success", device_code="devcode123", user_code="ABCD-EFGH", expires_in=600, interval=5))
        self.queue("poll", reply(status="success", token="tok_en-1", deprecation_key="AttackerKey+/0=", **dict(CONFIG, prefix60="2001:db8:dead::/60")))
        self.queue("config", config_reply)
        return self.run_script("enroll")

    def test_without_a_pinned_key_list_the_old_behaviour_stays_and_a_warning_is_logged(self):
        result = self.enroll()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no pinned key", result.stderr)
        self.assertEqual([c for c in self.calls() if "/config" in c], [], "no extra request, no nonce")

    def test_with_a_pinned_key_enrolment_applies_only_the_signed_config_and_keeps_the_pinned_key(self):
        result = self.enroll_pinned(self.signed())
        self.assertEqual(result.returncode, 0, result.stderr)
        uci = (self.fake / "uci").read_text()
        self.assertIn("hams_node.main.prefix60=2001:db8:ab00:10::/60", uci)
        self.assertNotIn("dead", uci, "the unsigned poll reply is never applied")
        self.assertEqual((self.state / "deprecation.pub").read_text().strip(), self.signer.public_line, "the key in the reply is not trusted")
        self.assertRegex([c for c in self.calls() if "/config" in c][0], r'"token":"tok_en-1","nonce":"[0-9a-f]{32}"')
        self.assertIn("hams-node-enroll fetch", self.cron.read_text())

    def test_with_a_pinned_key_a_bad_config_is_not_applied(self):
        other = Signer()
        cases = {
            "unsigned": reply(status="success", **CONFIG),
            "wrong key": self.signed(other),
            "replayed nonce": self.signed(nonce="0123456789abcdef0123456789abcdef"),
            "a status passed off as a config": self.signed(kind="status"),
        }
        for name, answer in cases.items():
            (self.fake / "uci").unlink(missing_ok=True)
            result = self.enroll_pinned(answer)
            self.assertEqual(result.returncode, 4, (name, result.stderr))
            self.assertFalse((self.fake / "uci").exists(), name)
            self.assertFalse((self.state / "deprecation.pub").exists(), name)

    def test_with_a_pinned_key_fetch_keeps_the_previous_config_unless_the_new_one_is_signed(self):
        self.assertEqual(self.enroll_pinned(self.signed()).returncode, 0)
        (self.fake / "uci").unlink()
        for answer in (reply(status="success", **dict(CONFIG, prefix60="2001:db8:dead::/60")), self.signed(Signer(), prefix60="2001:db8:dead::/60")):
            self.queue("config", answer)
            self.assertEqual(self.run_script("fetch").returncode, 4)
            self.assertFalse((self.fake / "uci").exists())
        self.queue("config", self.signed(prefix60="2001:db8:ab00:20::/60"))
        self.assertEqual(self.run_script("fetch").returncode, 0)
        self.assertIn("hams_node.main.prefix60=2001:db8:ab00:20::/60", (self.fake / "uci").read_text())

    def test_with_a_pinned_key_the_explicit_refusal_still_ends_the_enrolment(self):
        self.assertEqual(self.enroll_pinned(self.signed()).returncode, 0)
        self.queue("config", reply(status="error", message="refused"))
        self.assertEqual(self.run_script("fetch").returncode, 3)

    def test_a_rotation_list_accepts_either_pinned_key(self):
        later = Signer()
        self.pin(later, self.signer)
        self.assertEqual(self.enroll_pinned(self.signed()).returncode, 0)
        self.assertEqual((self.state / "deprecation.pub").read_text().strip(), self.signer.public_line)


if __name__ == "__main__":
    unittest.main()
