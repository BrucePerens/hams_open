# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the Tier B secret broker (zero_sudo/daemon/secret_broker/): the web account cannot read the
broker's credentials (startup refuses otherwise), a socket peer with the wrong uid is refused (real
SO_PEERCRED), no operation returns the secret (including an HTTPS provider that echoes the credential
back), the policy refusals, and the provisioning plan. Story: zero_sudo/docs/stories/secret_store_tier_b.md."""

import base64
import contextlib
import datetime
import errno
import http.server
import io
import json
import os
import shutil
import socket
import ssl
import subprocess
import struct
import tempfile
import threading
import time

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID
from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.daemon import secret_operations as ops
from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.daemon.secret_broker import broker as sb
from odoo.addons.zero_sudo.daemon.secret_broker import main as broker_main
from odoo.addons.zero_sudo.daemon.secret_broker import provision

SECRET = "sk_live_broker_secret_9d1c"
WEBHOOK = "whsec_broker_secret_77aa"
# The test provider is a real HTTPS server in this process; port 1 (default base_url) has nothing listening.
TEST_HOST = "localhost"  # burn-ignore-test-local-server: the provider the tests start on this machine
TEST_BIND = "127.0.0.1"  # burn-ignore-test-local-server: the provider the tests start on this machine


def _config(sock, extra=None):
    secrets = {
        "stripe.secret_key": {"operations": ["https_request"], "https": {
            "base_url": f"https://{TEST_HOST}:1", "methods": ["GET", "POST"], "path_prefixes": ["/v1/"],
            "auth": "bearer",
        }},
        "stripe.webhook_secret": {"operations": ["stripe_webhook_verify", "hmac_sign", "hmac_verify"]},
    }
    secrets.update(extra or {})
    return {"socket": sock, "allowed_users": ["odoo"], "deny_readers": ["odoo"], "credentials": "systemd",
            "secrets": secrets}


def _acl(*entries):
    raw = struct.pack("<I", 2)
    for tag, perm, qualifier in entries:
        raw += struct.pack("<HHI", tag, perm, qualifier & 0xFFFFFFFF)
    return raw


class _EchoHandler(http.server.BaseHTTPRequestHandler):
    """A provider that echoes the credential it received in the body and in a redirect."""

    def _answer(self):
        auth = self.headers.get("Authorization", "")
        body = json.dumps({"path": self.path, "auth": auth, "method": self.command}).encode()
        self.send_response(302 if self.path.endswith("/redirect") else 200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Location", f"https://elsewhere.example/?k={auth}")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_POST = _answer

    def log_message(self, *args):
        pass


def _tls_server(directory):
    """A real HTTPS server on this machine with a self-signed certificate; returns (server, ca_file)."""
    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, TEST_HOST)])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (
        x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
        .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(minutes=5))
        .not_valid_after(now + datetime.timedelta(hours=1))
        .add_extension(x509.SubjectAlternativeName([x509.DNSName(TEST_HOST)]), critical=False)
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .sign(key, hashes.SHA256())
    )
    cert_path, key_path = os.path.join(directory, "cert.pem"), os.path.join(directory, "key.pem")
    with open(cert_path, "wb") as handle:
        handle.write(cert.public_bytes(serialization.Encoding.PEM))
    with open(key_path, "wb") as handle:
        handle.write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                       serialization.NoEncryption()))
    server = http.server.ThreadingHTTPServer((TEST_BIND, 0), _EchoHandler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert_path, key_path)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()  # burn-ignore-test-daemon-thread: stopped by server.shutdown in the test's cleanup
    return server, cert_path


@tagged("post_install", "-at_install")
class TestSecretBrokerPolicy(BaseCase):
    def setUp(self):
        super().setUp()
        self.tmp = tempfile.mkdtemp(prefix="secret_broker_")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        with open(os.path.join(self.tmp, "stripe.secret_key"), "w") as handle:
            handle.write(SECRET)
        with open(os.path.join(self.tmp, "stripe.webhook_secret"), "w") as handle:
            handle.write(WEBHOOK)
        for name in ("stripe.secret_key", "stripe.webhook_secret"):
            os.chmod(os.path.join(self.tmp, name), 0o400)
        self.store = ss.SecretStore([ss.SystemdCredentialBackend(self.tmp)])
        self.os_uid = os.getuid()

    def _broker(self, config=None):
        return sb.Broker(config or _config(os.path.join(self.tmp, "b.sock")), self.store, {self.os_uid})

    def _ask(self, broker, op, **args):
        return broker.handle(json.dumps({"v": 1, "op": op, "args": args}).encode(), self.os_uid)

    # Tests [@ANCHOR: zero_sudo_secret_store_secret_store]
    def test_a_secret_that_has_gone_or_a_nested_request_gets_an_error_reply(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_handle]
        broker = self._broker()
        os.unlink(os.path.join(self.tmp, "stripe.webhook_secret"))
        self.assertEqual(
            self._ask(broker, "hmac_sign", name="stripe.webhook_secret", payload="p"),
            {"ok": False, "error": "secret_unavailable"},
        )
        os.chmod(os.path.join(self.tmp, "stripe.secret_key"), 0o404)
        self.assertEqual(
            self._ask(broker, "https_request", name="stripe.secret_key", method="GET", path="/v1/x"),
            {"ok": False, "error": "secret_unavailable"},
        )
        nested = b'{"v": 1, "op": "hmac_sign", "args": ' + b"[" * 100000 + b"]" * 100000 + b"}"
        self.assertEqual(broker.handle(nested, self.os_uid), {"ok": False, "error": "bad_request"})

    # Tests [@ANCHOR: zero_sudo_broker_broker_config_error]
    def test_config_validation(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_validate_config]
        # Tests [@ANCHOR: zero_sudo:secret_broker_validate_https]
        good = _config("/run/x.sock")
        self.assertIs(sb.validate_config(good), good)
        https = good["secrets"]["stripe.secret_key"]["https"]
        bad_https = [
            {"base_url": "http://api.example"}, {"base_url": "https://u:p@api.example"},
            {"base_url": "https://api.example/v1"}, {"methods": ["TRACE"]}, {"path_prefixes": ["v1/"]},
            {"auth": "basic"}, {"auth": "header"}, {"auth": "query"}, {"timeout": 0},
        ]
        for change in bad_https:
            config = json.loads(json.dumps(good))
            config["secrets"]["stripe.secret_key"]["https"] = dict(https, **change)
            with self.assertRaises(sb.BrokerConfigError, msg=change):
                sb.validate_config(config)
        bad_top = [
            {"socket": "relative.sock"}, {"allowed_users": []}, {"deny_readers": None},
            {"credentials": "relative"}, {"secrets": {}},
            {"secrets": {"a.key": {"operations": ["get_secret"]}}},
            {"secrets": {"a.key": {"operations": ["https_request"]}}},
        ]
        for change in bad_top:
            with self.assertRaises(sb.BrokerConfigError, msg=change):
                sb.validate_config(dict(good, **change))
        with self.assertRaises(sb.BrokerConfigError):
            sb.validate_config([])

    def test_no_operation_returns_the_secret(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_init]
        # Tests [@ANCHOR: zero_sudo:secret_broker_handle]
        # Tests [@ANCHOR: zero_sudo:secret_broker_run]
        # Tests [@ANCHOR: zero_sudo:secret_broker_https_call]
        server, ca_file = _tls_server(self.tmp)
        self.addCleanup(server.shutdown)
        config = _config(os.path.join(self.tmp, "b.sock"))
        https = config["secrets"]["stripe.secret_key"]["https"]
        https.update(base_url=f"https://{TEST_HOST}:{server.server_address[1]}", ca_file=ca_file)
        broker = self._broker(config)
        replies = [
            self._ask(broker, "hmac_sign", name="stripe.webhook_secret", payload="p"),
            self._ask(broker, "hmac_verify", name="stripe.webhook_secret", payload="p", signature="00"),
            self._ask(broker, "stripe_webhook_verify", name="stripe.webhook_secret", payload="b",
                      signature_header="t=1,v1=00"),
            self._ask(broker, "https_request", name="stripe.secret_key", method="GET", path="/v1/charges?limit=1"),
            self._ask(broker, "https_request", name="stripe.secret_key", method="POST", path="/v1/redirect",
                      body="a=1", headers={"Idempotency-Key": "k1"}),
        ]
        self.assertFalse(replies[2]["result"])  # an old timestamp is outside the broker's own tolerance
        for reply in replies:
            self.assertTrue(reply["ok"], reply)
            self.assertNotIn(SECRET, json.dumps(reply))
            self.assertNotIn(WEBHOOK, json.dumps(reply))
        self.assertEqual(replies[0]["result"], ops.hmac_sign(WEBHOOK, "p"))
        echoed = json.loads(replies[3]["result"]["body"])
        self.assertEqual(echoed["auth"], f"Bearer {sb.REDACTED}")  # the provider saw it; the caller does not
        self.assertEqual(echoed["path"], "/v1/charges?limit=1")
        self.assertEqual(replies[4]["result"]["status"], 302)
        self.assertIn(sb.REDACTED, replies[4]["result"]["location"])

    def test_basic_and_header_auth_are_redacted_too(self):
        server, ca_file = _tls_server(self.tmp)
        self.addCleanup(server.shutdown)
        base = {"base_url": f"https://{TEST_HOST}:{server.server_address[1]}", "ca_file": ca_file,
                "methods": ["GET"], "path_prefixes": ["/api"]}
        basic = sb.https_call(dict(base, auth="basic", username="key_id"), SECRET, "GET", "/api/x")
        self.assertNotIn(SECRET, json.dumps(basic))
        self.assertNotIn(base64.b64encode(f"key_id:{SECRET}".encode()).decode(), json.dumps(basic))
        header = sb.https_call(dict(base, auth="header", header_name="X-Api-Key"), SECRET, "GET", "/api")
        self.assertEqual(json.loads(header["body"])["auth"], "")

    def test_a_provider_response_above_the_size_limit_is_refused_not_relayed(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_https_call]
        server, ca_file = _tls_server(self.tmp)
        self.addCleanup(server.shutdown)
        base = {"base_url": f"https://{TEST_HOST}:{server.server_address[1]}", "ca_file": ca_file,
                "methods": ["GET"], "path_prefixes": ["/api"], "auth": "bearer"}
        self.assertEqual(sb.https_call(base, SECRET, "GET", "/api")["status"], 200)
        original = sb.MAX_RESPONSE_BYTES
        self.addCleanup(setattr, sb, "MAX_RESPONSE_BYTES", original)
        sb.MAX_RESPONSE_BYTES = 10  # the echoed body is longer than this
        with self.assertRaises(sb.Refused) as caught:
            sb.https_call(base, SECRET, "GET", "/api")
        self.assertEqual(str(caught.exception), "response_too_large")

    def test_refusals(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_refused_init]
        broker = self._broker()
        cases = [
            (b"not json", "bad_request"),
            (json.dumps({"v": 2, "op": "hmac_sign", "args": {}}).encode(), "bad_request"),
            (json.dumps({"v": 1, "op": "get_secret", "args": {"name": "stripe.secret_key"}}).encode(),
             "unknown_operation"),
            (json.dumps({"v": 1, "op": "hmac_sign", "args": {"name": "nope"}}).encode(), "unknown_secret"),
            (json.dumps({"v": 1, "op": "hmac_sign", "args": {"name": "stripe.secret_key", "payload": "p"}}).encode(),
             "operation_not_allowed"),
            (json.dumps({"v": 1, "op": "hmac_sign",
                         "args": {"name": "stripe.webhook_secret", "payload": "p", "key": "x"}}).encode(),
             "bad_request"),
            (json.dumps({"v": 1, "op": "hmac_sign",
                         "args": {"name": "stripe.webhook_secret", "payload": "p", "digest": "md5"}}).encode(),
             "bad_request"),
        ]
        for raw, code in cases:
            self.assertEqual(broker.handle(raw, self.os_uid), {"ok": False, "error": code}, raw)
        https_cases = [
            ({"method": "DELETE", "path": "/v1/x"}, "method_not_allowed"),
            ({"method": "GET", "path": "/v2/x"}, "path_not_allowed"),
            ({"method": "GET", "path": "/v1/x", "headers": {"Authorization": "Bearer mine"}}, "header_not_allowed"),
            ({"method": "GET", "path": "/v1/x", "headers": {"Accept": "a\r\nX: y"}}, "header_not_allowed"),
            ({"method": "POST", "path": "/v1/x", "body": {"a": 1}}, "body_not_string"),
        ]
        for args, code in https_cases:
            reply = self._ask(broker, "https_request", name="stripe.secret_key", **args)
            self.assertEqual(reply, {"ok": False, "error": code}, args)
        # A peer that is not allowed is refused before the request is even parsed.
        self.assertEqual(broker.handle(b"anything", self.os_uid + 1), {"ok": False, "error": "peer_not_allowed"})
        self.assertEqual(broker.handle(b"anything", None)["error"], "peer_not_allowed")
        self.assertEqual(broker.handle(b"anything", True)["error"], "peer_not_allowed")

    def test_upstream_failure_is_a_code(self):
        config = _config(os.path.join(self.tmp, "b.sock"))
        broker = self._broker(config)  # base_url port 1 of TEST_HOST: nothing listens there
        reply = self._ask(broker, "https_request", name="stripe.secret_key", method="GET", path="/v1/x")
        self.assertEqual(reply, {"ok": False, "error": "upstream_failed"})

    def test_caller_cannot_choose_now_for_the_webhook_window(self):
        broker = self._broker(_config(os.path.join(self.tmp, "b.sock")))
        header = "t=1,v1=" + ops.hmac_sign(WEBHOOK, "1.b")
        # Valid signature at t=1: only a caller-chosen `now` could make it pass; the broker refuses the argument.
        reply = self._ask(broker, "stripe_webhook_verify", name="stripe.webhook_secret", payload="b",
                          signature_header=header, now=1)
        self.assertEqual(reply, {"ok": False, "error": "bad_request"})
        reply = self._ask(broker, "stripe_webhook_verify", name="stripe.webhook_secret", payload="b",
                          signature_header=header)
        self.assertEqual(reply, {"ok": True, "result": False})
        now = int(time.time())
        fresh = f"t={now},v1=" + ops.hmac_sign(WEBHOOK, f"{now}.b")
        self.assertTrue(self._ask(broker, "stripe_webhook_verify", name="stripe.webhook_secret", payload="b",
                                  signature_header=fresh)["result"])
        stale = f"t={now - 400},v1=" + ops.hmac_sign(WEBHOOK, f"{now - 400}.b")
        self.assertFalse(self._ask(broker, "stripe_webhook_verify", name="stripe.webhook_secret", payload="b",
                                   signature_header=stale)["result"])
        self.assertTrue(self._ask(broker, "stripe_webhook_verify", name="stripe.webhook_secret", payload="b",
                                  signature_header=stale, tolerance=500)["result"])

    # Tests [@ANCHOR: zero_sudo_broker_refused]
    def test_paths(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_check_path]
        prefixes = ["/v1/", "/api"]
        for good in ("/v1/charges", "/v1/charges?limit=3&a=b", "/api", "/api/x"):
            self.assertEqual(sb.check_path(good, prefixes), good)
        for bad in ("v1/x", "//evil/v1/", "/v1/../admin", "/v1/%2e%2e/x", "/v1/a%2Fb", "/v1//x", "/apix",
                    "/v1/x@evil", "/v1/x#f", "/v1/x\\y", "/v1/x y", "/v1/\nx", "/v2/x", None,
                    "/v1/a%5cb", "/v1/a%5Cb", "/v1/a%00b", "/v1/a%252fb", 
                    "/v1/a%255cb", "/v1/a%25252fb", "/v1/a%2525252fb", "/v1/%252e%252e/x"):
            with self.assertRaises(sb.Refused, msg=bad):
                sb.check_path(bad, prefixes)

    def test_redact_and_scrub(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_redact]
        # Tests [@ANCHOR: zero_sudo:secret_broker_scrub]
        self.assertEqual(sb.redact(f"a{SECRET}b", SECRET), f"a{sb.REDACTED}b")
        self.assertEqual(sb.redact("abc", "ab"), "abc")  # too short to search for
        self.assertEqual(sb.redact("xPAIRx", SECRET, ["PAIR"]), f"x{sb.REDACTED}x")
        nested = {"k": [SECRET, {SECRET: 1}], "n": 3, "b": True, "none": None}
        self.assertNotIn(SECRET, json.dumps(sb.scrub(nested, SECRET)))
        self.assertEqual(sb.scrub(nested, SECRET)["n"], 3)

    def test_secrets_present_at_start(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_check_secrets_present]
        self._broker().check_secrets_present()
        os.chmod(os.path.join(self.tmp, "stripe.webhook_secret"), 0o600)
        with open(os.path.join(self.tmp, "stripe.webhook_secret"), "w") as handle:
            handle.write("short")
        with self.assertRaises(sb.BrokerConfigError):
            self._broker().check_secrets_present()
        os.unlink(os.path.join(self.tmp, "stripe.webhook_secret"))
        with self.assertRaises(ss.SecretNotFound):
            self._broker().check_secrets_present()


@tagged("post_install", "-at_install")
class TestSecretBrokerIsolation(BaseCase):
    """The two isolation properties: the web account cannot read the broker's secrets, and a socket peer
    with the wrong uid is refused."""

    def setUp(self):
        super().setUp()
        self.tmp = tempfile.mkdtemp(prefix="secret_broker_")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.os_uid = os.getuid()

    def test_a_request_above_the_size_limit_gets_request_too_large_and_is_never_handled(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_serve_connection]
        broker = sb.Broker(_config(os.path.join(self.tmp, "b.sock")), ss.SecretStore([ss.SystemdCredentialBackend(self.tmp)]), {self.os_uid})
        handled = []
        broker.handle = lambda data, uid: handled.append(data) or {"ok": True}
        client, server_side = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(client.close)
        self.addCleanup(server_side.close)
        client.settimeout(5)
        server_side.settimeout(5)
        sender = threading.Thread(target=lambda: self._send_quietly(client, b"x" * (sb.MAX_REQUEST_BYTES + 70000)))  # burn-ignore-test-daemon-thread: joined below
        sender.start()
        broker.serve_connection(server_side)
        sender.join(10)
        client.settimeout(5)
        reply = b""
        try:
            while not reply.endswith(b"\n"):
                chunk = client.recv(65536)
                if not chunk:
                    break
                reply += chunk
        except OSError as exc:  # the broker may reset after its answer; the answer itself is what is asserted
            reply_error = exc
            self.assertTrue(reply, f"no reply before the connection ended: {reply_error}")
        self.assertEqual(json.loads(reply), {"ok": False, "error": "request_too_large"})
        self.assertEqual(handled, [])

    def test_a_peer_whose_kernel_reported_uid_is_not_allowed_is_refused_without_reading_its_request(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_serve_connection]
        store = ss.SecretStore([ss.SystemdCredentialBackend(self.tmp)])
        broker = sb.Broker(_config(os.path.join(self.tmp, "b.sock")), store, {self.os_uid + 1})
        handled = []
        broker.handle = lambda data, uid: handled.append(data) or {"ok": True}
        client, server_side = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(client.close)
        client.settimeout(5)
        client.sendall(b'{"v": 1, "op": "hmac_sign", "args": {}}\n')
        client.shutdown(socket.SHUT_WR)  # the broker waits for the caller's EOF before it closes
        broker.serve_connection(server_side)
        self.assertEqual(json.loads(client.recv(65536)), {"ok": False, "error": "peer_not_allowed"})
        self.assertEqual(handled, [])

    @staticmethod
    def _send_quietly(sock, data):
        try:
            sock.sendall(data)
            sock.shutdown(2 - 1)  # SHUT_WR: end of the request
        except OSError as exc:  # the broker stops reading once it has refused; a reset here is expected
            return exc
        return None

    def test_acl_entries(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_acl_grants_read]
        base = [(0x01, 6, -1), (0x04, 4, -1), (0x20, 0, -1)]
        self.assertTrue(sb.acl_grants_read(_acl(*base, (0x02, 4, 1234), (0x10, 4, -1)), 1234, set()))
        self.assertFalse(sb.acl_grants_read(_acl(*base, (0x02, 4, 1234), (0x10, 0, -1)), 1234, set()))
        self.assertFalse(sb.acl_grants_read(_acl(*base, (0x02, 4, 1234), (0x10, 4, -1)), 999, set()))
        self.assertTrue(sb.acl_grants_read(_acl(*base, (0x08, 4, 77), (0x10, 4, -1)), 999, {77}))
        self.assertTrue(sb.acl_grants_read(b"\x02\x00", 1, set()))  # malformed: counts as readable

    def test_readable_by_fails_closed_on_an_unexpected_getxattr_error(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_readable_by]
        path = os.path.join(self.tmp, "secrets")
        with open(path, "w") as handle:
            handle.write("a=b\n")
        os.chmod(path, 0o600)

        def failing(errno_value):
            def getxattr(p, n):
                raise OSError(errno_value, "boom")
            return getxattr
        other = self.os_uid + 1
        self.assertFalse(sb.readable_by(path, other, set(), getxattr=failing(errno.ENODATA)))
        self.assertFalse(sb.readable_by(path, other, set(), getxattr=failing(errno.ENOTSUP)))
        for code in (errno.EACCES, errno.EIO):
            self.assertTrue(sb.readable_by(path, other, set(), getxattr=failing(code)))

    def test_readable_by(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_readable_by]
        path = os.path.join(self.tmp, "secrets")
        with open(path, "w") as handle:
            handle.write("a=b\n")
        gid = os.stat(path).st_gid
        os.chmod(path, 0o600)
        other = self.os_uid + 1
        self.assertTrue(sb.readable_by(path, self.os_uid, set()))
        self.assertTrue(sb.readable_by(path, 0, set()))
        self.assertFalse(sb.readable_by(path, other, {gid}))
        os.chmod(path, 0o640)
        self.assertTrue(sb.readable_by(path, other, {gid}))
        self.assertFalse(sb.readable_by(path, other, set()))
        os.chmod(path, 0o604)
        self.assertTrue(sb.readable_by(path, other, set()))
        os.chmod(path, 0o600)
        acl = _acl((0x01, 6, -1), (0x02, 4, other), (0x04, 0, -1), (0x10, 4, -1), (0x20, 0, -1))
        self.assertTrue(sb.readable_by(path, other, set(), getxattr=lambda p, n: acl))

    def test_web_account_cannot_read_the_credentials(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_check_not_readable]
        # Tests [@ANCHOR: zero_sudo:secret_broker_account_ids]
        path = os.path.join(self.tmp, "secrets")
        with open(path, "w") as handle:
            handle.write("a=b\n")
        os.chmod(path, 0o600)
        web_other = {"odoo": (self.os_uid + 1, set())}
        sb.check_not_readable([path], ["odoo"], web_other.get)
        # The same file is refused when the web account is its owner, or reaches it through a group.
        with self.assertRaises(sb.BrokerConfigError):
            sb.check_not_readable([path], ["odoo"], {"odoo": (self.os_uid, set())}.get)
        os.chmod(path, 0o640)
        with self.assertRaises(sb.BrokerConfigError):
            sb.check_not_readable([path], ["odoo"], {"odoo": (self.os_uid + 1, {os.stat(path).st_gid})}.get)
        uid, gids = sb.account_ids(self.os_uid)
        self.assertEqual(uid, self.os_uid)
        self.assertIn(os.getgid(), gids)

    # Tests [@ANCHOR: zero_sudo_secret_store_broker_client]
    # Tests [@ANCHOR: zero_sudo_secret_store_systemd_credential_backend]
    # Tests [@ANCHOR: zero_sudo_secret_store_secret_store_error]
    def test_wrong_peer_uid_is_refused_on_the_real_socket(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_peer_credentials]
        # Tests [@ANCHOR: zero_sudo:secret_broker_serve_connection]
        # Tests [@ANCHOR: zero_sudo:secret_broker_serve]
        creds = os.path.join(self.tmp, "creds")
        os.mkdir(creds)
        with open(os.path.join(creds, "stripe.webhook_secret"), "w") as handle:
            handle.write(WEBHOOK)
        os.chmod(os.path.join(creds, "stripe.webhook_secret"), 0o400)
        store = ss.SecretStore([ss.SystemdCredentialBackend(creds)])
        results = {}
        for label, allowed in (("me", {self.os_uid}), ("other", {self.os_uid + 1})):
            sock = os.path.join(self.tmp, f"{label}.sock")
            stop = threading.Event()
            broker = sb.Broker(_config(sock), store, allowed)
            thread = threading.Thread(target=broker.serve, args=(stop,), daemon=True)  # burn-ignore-test-daemon-thread: stop.set() then join(5) below
            thread.start()
            self.addCleanup(stop.set)
            for _ in range(100):
                if os.path.exists(sock):
                    break
                threading.Event().wait(0.02)
            self.assertEqual(oct(os.stat(sock).st_mode & 0o777), "0o660")
            client = ss.BrokerClient(sock)
            try:
                results[label] = client.call("hmac_sign", name="stripe.webhook_secret", payload="p")
            except ss.SecretStoreError as exc:
                results[label] = str(exc)
            stop.set()
            thread.join(5)
        self.assertEqual(results["me"], ops.hmac_sign(WEBHOOK, "p"))
        self.assertIn("peer_not_allowed", results["other"])

    def test_startup_checks(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_main_build]
        creds = os.path.join(self.tmp, "creds")
        os.mkdir(creds)
        path = os.path.join(creds, "secrets")
        with open(path, "w") as handle:
            handle.write(f"stripe.webhook_secret={WEBHOOK}\nstripe.secret_key={SECRET}\n")
        os.chmod(path, 0o600)
        config = _config(os.path.join(self.tmp, "b.sock"))
        env = {"CREDENTIALS_DIRECTORY": creds}
        web_other = {"odoo": (self.os_uid + 1, set())}.get
        built = broker_main.build(config, env, self.os_uid, web_other)
        self.assertEqual(built.allowed_uids, frozenset({self.os_uid + 1}))
        refusals = [
            (config, env, 0, web_other),                                      # root
            (config, {}, self.os_uid, web_other),                                # no LoadCredential
            (config, env, self.os_uid, {"odoo": (self.os_uid, set())}.get),         # web account can read it
            (dict(config, credentials=path), env, self.os_uid, web_other),      # not root-owned
        ]
        for args in refusals:
            with self.assertRaises((broker_main.broker.BrokerConfigError, broker_main.secret_store.SecretStoreError)):
                broker_main.build(*args)

    def test_main_refuses_to_start_without_a_valid_config(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_main]
        missing = os.path.join(self.tmp, "absent.json")
        with self.assertLogs(level="ERROR"):
            self.assertEqual(broker_main.main(["--config", missing]), 2)


@tagged("post_install", "-at_install")
class TestSecretBrokerProvisioning(BaseCase):
    def test_unit_and_dropin(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_unit_text]
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_odoo_dropin_text]
        unit = provision.unit_text("/opt/zs/daemon")
        for line in ("User=hams_secret_broker", "LoadCredential=secrets:/etc/hams_secret_broker/secrets",
                     "ExecStart=/usr/bin/python3 /opt/zs/daemon/secret_broker/main.py", "NoNewPrivileges=yes",
                     "ProtectSystem=strict", "RuntimeDirectoryMode=0750", "CapabilityBoundingSet="):
            self.assertIn(line, unit)
        self.assertNotIn("EnvironmentFile", unit)  # no secret in the broker's environment either
        dropin = provision.odoo_dropin_text()
        self.assertNotIn("HAMS_SECRET_STORE", dropin)  # a broker-only spec would drop the Tier A store that core secrets need
        self.assertIn("SupplementaryGroups=hams_secret_broker_clients", dropin)

    def test_plan_keeps_the_secrets_file_root_only_and_never_overwrites_it(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_plan]
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_default_config]
        steps = provision.plan("odoo", "/opt/zs/daemon", "odoo.service")
        by_path = {s[1]: s for s in steps if s[0] in ("file", "file_if_absent", "dir")}
        secrets = by_path["/etc/hams_secret_broker/secrets"]
        self.assertEqual((secrets[0], secrets[2]), ("file_if_absent", 0o600))
        self.assertEqual(by_path["/etc/hams_secret_broker/broker.json"][0], "file_if_absent")
        config = json.loads(by_path["/etc/hams_secret_broker/broker.json"][3])
        self.assertEqual(config, provision.default_config("odoo"))
        self.assertEqual(config["deny_readers"], ["odoo"])
        self.assertIn("/etc/systemd/system/odoo.service.d/hams_secret_broker.conf", by_path)
        commands = [s[1] for s in steps if s[0].startswith("run")]
        self.assertIn(["usermod", "--append", "--groups", "hams_secret_broker_clients", "odoo"], commands)
        # The web account joins only the clients group; nothing grants it the broker's own account or files.
        self.assertFalse(any("hams_secret_broker" in c and c[0] == "usermod" and c[-1] != "odoo" for c in commands))
        self.assertNotIn("/etc/systemd/system/odoo.service.d/hams_secret_broker.conf",
                         {s[1] for s in provision.plan("odoo", "/x") if s[0] in ("file", "file_if_absent")})

    def test_apply_dry_run_and_keep_existing(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_apply]
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_main]
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_account_exists]
        self.assertTrue(provision.account_exists("root"))
        self.assertFalse(provision.account_exists("no_such_account_for_secret_broker"))
        tmp = tempfile.mkdtemp(prefix="secret_broker_")
        self.addCleanup(shutil.rmtree, tmp, True)
        existing = os.path.join(tmp, "secrets")
        with open(existing, "w") as handle:
            handle.write("keep me\n")
        calls = []
        steps = [("run", ["true"]), ("run_if_no_user", ["useradd", "x"]), ("dir", os.path.join(tmp, "d"), 0o755),
                 ("file_if_absent", existing, 0o600, "new"), ("file", os.path.join(tmp, "unit"), 0o644, "u")]
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            provision.apply(steps, True, run=lambda *a, **k: calls.append(a), user_exists=lambda n: True)
        self.assertEqual(calls, [])
        self.assertIn(f"keep: {existing} exists", out.getvalue())
        self.assertIn("keep: account x exists", out.getvalue())
        self.assertFalse(os.path.exists(os.path.join(tmp, "unit")))
        with open(existing) as handle:
            self.assertEqual(handle.read(), "keep me\n")
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(provision.main(["--web-user", "odoo", "--code-dir", tmp, "--dry-run"]), 0)
            if os.geteuid() != 0:
                self.assertEqual(provision.main(["--web-user", "odoo", "--code-dir", tmp]), 2)

    def test_apply_writes_files_with_the_stated_mode_and_refuses_to_follow_a_symlink(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_apply]
        tmp = tempfile.mkdtemp(prefix="secret_broker_prov_")
        self.addCleanup(shutil.rmtree, tmp, True)
        keep = os.path.join(tmp, "etc", "broker.json")
        unit = os.path.join(tmp, "units", "broker.service")
        steps = [("dir", os.path.join(tmp, "creds"), 0o700), ("file_if_absent", keep, 0o600, "{}"),
                 ("file", unit, 0o644, "unit-v1")]
        chowned = []
        self.addCleanup(setattr, os, "chown", os.chown)  # this test is not root: record the call instead of making it
        os.chown = lambda path, uid, gid: chowned.append((path, uid, gid))
        with contextlib.redirect_stdout(io.StringIO()):
            provision.apply(steps, False)
            self.assertEqual(os.stat(os.path.join(tmp, "creds")).st_mode & 0o777, 0o700)
            self.assertEqual(os.stat(keep).st_mode & 0o777, 0o600)
            self.assertEqual(os.stat(unit).st_mode & 0o777, 0o644)
            self.assertIn((unit, 0, 0), chowned)  # root-owned
            with open(keep, "w") as handle:
                handle.write("operator edit")
            provision.apply([("file_if_absent", keep, 0o600, "{}"), ("file", unit, 0o644, "unit-v2")], False)
            with open(keep) as handle:
                self.assertEqual(handle.read(), "operator edit")  # an existing config is never rewritten
            with open(unit) as handle:
                self.assertEqual(handle.read(), "unit-v2")  # a unit is refreshed
            victim = os.path.join(tmp, "victim")
            with open(victim, "w") as handle:
                handle.write("untouched")
            link = os.path.join(tmp, "units", "link.service")
            os.symlink(victim, link)
            with self.assertRaises(OSError):
                provision.apply([("file", link, 0o644, "pwned")], False)
        with open(victim) as handle:
            self.assertEqual(handle.read(), "untouched")

    def test_a_failing_provisioning_command_stops_the_run(self):
        # Tests [@ANCHOR: zero_sudo:secret_broker_provision_apply]
        ran = []

        def run(cmd, check):
            ran.append(cmd)
            raise subprocess.CalledProcessError(1, cmd)

        with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(subprocess.CalledProcessError):
            provision.apply([("run", ["useradd", "x"]), ("run", ["systemctl", "daemon-reload"])], False, run=run)
        self.assertEqual(ran, [["useradd", "x"]])
