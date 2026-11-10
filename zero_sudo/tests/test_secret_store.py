# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the secret store (zero_sudo/daemon/secret_store.py) and its operations
(zero_sudo/daemon/secret_operations.py): the provider interface, each Tier A backend (systemd
credential, root-owned credentials file, encrypted fallback), the Tier B client and the spec parser.
Story: zero_sudo/docs/stories/secret_store_tier_a.md."""

import base64
import json
import os
import shutil
import socket
import tempfile
import threading
import time

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.daemon import secret_operations as ops
from odoo.addons.zero_sudo.daemon import secret_store as ss

SECRET = "sk_test_do_not_leak_4f9a"


def _write(path, text, mode=0o640):
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    os.chmod(path, mode)
    return path


class _FakeBroker:
    """A unix-socket server that answers each request with `reply(request)` and records the requests."""

    def __init__(self, directory, reply):
        self.path = os.path.join(directory, "broker.sock")
        self.reply = reply
        self.requests = []
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(self.path)
        self.server.listen(4)
        self.thread = threading.Thread(target=self._serve, daemon=True)  # burn-ignore-test-daemon-thread: ends when close() closes the listening socket
        self.thread.start()

    def _serve(self):
        while True:
            try:
                conn, _ = self.server.accept()
            except OSError:
                return
            with conn:
                data = b""
                while not data.endswith(b"\n"):
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                request = json.loads(data)
                self.requests.append(request)
                conn.sendall(json.dumps(self.reply(request)).encode() + b"\n")

    def close(self):
        self.server.close()


@tagged("post_install", "-at_install")
class TestSecretStoreTierA(BaseCase):
    def setUp(self):
        super().setUp()
        self.tmp = tempfile.mkdtemp(prefix="secret_store_")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.os_uid = os.getuid()

    def test_names_are_checked(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_check_name]
        self.assertEqual(ss.check_name("stripe.secret_key"), "stripe.secret_key")
        for bad in ("", "../etc/passwd", "a/b", "Upper", "a..b", "x" * 101, None, ".hidden"):
            with self.assertRaises(ValueError, msg=bad):
                ss.check_name(bad)

    def test_private_file_rules(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_check_private_file]
        good = _write(os.path.join(self.tmp, "good"), "a=b\n", 0o640)
        self.assertEqual(ss.check_private_file(good, self.os_uid).st_uid, self.os_uid)
        for mode in (0o644, 0o660, 0o604):
            os.chmod(good, mode)
            with self.assertRaises(ss.InsecureSecretSource, msg=oct(mode)):
                ss.check_private_file(good, self.os_uid)
        os.chmod(good, 0o600)
        if self.os_uid != 0:
            # Root-owned is the default: a file the reading account owns could be rewritten by it.
            with self.assertRaises(ss.InsecureSecretSource):
                ss.check_private_file(good)
        link = os.path.join(self.tmp, "link")
        os.symlink(good, link)
        with self.assertRaises(ss.InsecureSecretSource):
            ss.check_private_file(link, self.os_uid)

    def test_read_small_file_refuses_symlinks_and_large_files(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_read_small_file]
        small = _write(os.path.join(self.tmp, "small"), "abc")
        self.assertEqual(ss.read_small_file(small), b"abc")
        link = os.path.join(self.tmp, "link")
        os.symlink(small, link)
        with self.assertRaises(OSError):
            ss.read_small_file(link)
        big = _write(os.path.join(self.tmp, "big"), "x" * (ss.MAX_SECRET_BYTES + 1))
        with self.assertRaises(ss.SecretStoreError):
            ss.read_small_file(big)

    def test_credentials_text_parsing_never_echoes_a_value(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_parse_credentials_text]
        parsed = ss.parse_credentials_text(f"# comment\n\nstripe.secret_key = {SECRET}\nb=x=y\n")
        self.assertEqual(parsed, {"stripe.secret_key": SECRET, "b": "x=y"})
        for text in (f"no equals {SECRET}", f"Bad Name={SECRET}", f"a={SECRET}\na={SECRET}"):
            with self.assertRaises(ss.SecretStoreError) as caught:
                ss.parse_credentials_text(text)
            self.assertNotIn(SECRET, str(caught.exception))

    def test_systemd_credential_backend(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_systemd_init]
        # Tests [@ANCHOR: zero_sudo:secret_store_systemd_get]
        _write(os.path.join(self.tmp, "stripe.secret_key"), SECRET + "\n", 0o440)
        backend = ss.SystemdCredentialBackend(self.tmp)
        self.assertEqual(backend.get("stripe.secret_key"), SECRET)
        self.assertIsNone(backend.get("absent"))
        with self.assertRaises(ValueError):
            backend.get("../stripe.secret_key")
        _write(os.path.join(self.tmp, "loose"), SECRET, 0o444)
        with self.assertRaises(ss.InsecureSecretSource):
            backend.get("loose")

    def test_credentials_file_backend(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_file_init]
        # Tests [@ANCHOR: zero_sudo:secret_store_file_get]
        path = _write(os.path.join(self.tmp, "secrets"), f"stripe.secret_key={SECRET}\n", 0o640)
        backend = ss.CredentialsFileBackend(path, owner_uid=self.os_uid)
        self.assertEqual(backend.get("stripe.secret_key"), SECRET)
        self.assertIsNone(backend.get("other"))
        os.chmod(path, 0o644)  # loosened after start: refused on the next read, not trusted
        with self.assertRaises(ss.InsecureSecretSource):
            backend.get("stripe.secret_key")
        if self.os_uid != 0:
            os.chmod(path, 0o640)
            with self.assertRaises(ss.InsecureSecretSource):
                ss.CredentialsFileBackend(path).get("stripe.secret_key")

    def test_encrypted_backend_round_trip_and_binding(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_encrypted_init]
        # Tests [@ANCHOR: zero_sudo:secret_store_encrypted_get]
        # Tests [@ANCHOR: zero_sudo:secret_store_encrypt_value]
        # Tests [@ANCHOR: zero_sudo:secret_store_decrypt_value]
        # Tests [@ANCHOR: zero_sudo:secret_store_key_from_file]
        # Tests [@ANCHOR: zero_sudo:secret_store_json_file_lookup]
        key = os.urandom(32)
        key_path = _write(os.path.join(self.tmp, "key"), base64.urlsafe_b64encode(key).decode(), 0o600)
        token = ss.encrypt_value(key, "stripe.secret_key", SECRET)
        self.assertNotIn(SECRET, token)
        json_path = _write(
            os.path.join(self.tmp, "values.json"),
            json.dumps({"stripe.secret_key": token, "moved": token}), 0o644,
        )
        backend = ss.EncryptedBackend(ss.key_from_file(key_path, self.os_uid), ss.json_file_lookup(json_path))
        self.assertEqual(backend.get("stripe.secret_key"), SECRET)
        self.assertIsNone(backend.get("absent"))
        with self.assertRaises(ss.SecretStoreError):  # a token copied to another name does not decrypt
            backend.get("moved")
        with self.assertRaises(ss.SecretStoreError):
            ss.decrypt_value(os.urandom(32), "stripe.secret_key", token)
        with self.assertRaises(ss.SecretStoreError):
            ss.decrypt_value(key, "stripe.secret_key", "xx1." + token[4:])
        with self.assertRaises(ValueError):
            ss.encrypt_value(b"short", "a", "b")
        self.assertIsNone(ss.json_file_lookup(os.path.join(self.tmp, "none.json"))("a"))
        _write(json_path, "[1]", 0o644)
        with self.assertRaises(ss.SecretStoreError):
            ss.json_file_lookup(json_path)("a")
        _write(key_path, base64.urlsafe_b64encode(b"x" * 16).decode(), 0o600)
        with self.assertRaises(ss.SecretStoreError):
            ss.key_from_file(key_path, self.os_uid)()

    def test_store_order_missing_secret_and_require(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_init]
        # Tests [@ANCHOR: zero_sudo:secret_store_get]
        # Tests [@ANCHOR: zero_sudo:secret_store_require]
        first = os.path.join(self.tmp, "first")
        os.mkdir(first)
        _write(os.path.join(first, "a"), "from-systemd", 0o400)
        path = _write(os.path.join(self.tmp, "secrets"), "a=from-file\nb=only-file\n", 0o600)
        store = ss.SecretStore([ss.SystemdCredentialBackend(first), ss.CredentialsFileBackend(path, self.os_uid)])
        self.assertEqual(store.get("a"), "from-systemd")
        self.assertEqual(store.get("b"), "only-file")
        store.require("a", "b")
        with self.assertRaises(ss.SecretNotFound) as caught:
            store.require("a", "c")
        self.assertIn("systemd, file", str(caught.exception))
        with self.assertRaises(ss.SecretNotFound):
            ss.SecretStore().get("a")

    def test_local_operations_match_the_shared_code(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_hmac_sign]
        # Tests [@ANCHOR: zero_sudo:secret_store_hmac_verify]
        # Tests [@ANCHOR: zero_sudo:secret_store_stripe_webhook_verify]
        # Tests [@ANCHOR: zero_sudo:secret_store_https_request]
        _write(os.path.join(self.tmp, "whsec"), SECRET, 0o400)
        store = ss.SecretStore([ss.SystemdCredentialBackend(self.tmp)])
        signature = store.hmac_sign("whsec", "payload")
        self.assertEqual(signature, ops.hmac_sign(SECRET, "payload"))
        self.assertTrue(store.hmac_verify("whsec", "payload", signature))
        self.assertFalse(store.hmac_verify("whsec", "payload!", signature))
        now = int(time.time())
        header = f"t={now},v1={ops.hmac_sign(SECRET, f'{now}.body')}"
        self.assertTrue(store.stripe_webhook_verify("whsec", "body", header, now=now))
        with self.assertRaises(ss.SecretStoreError):
            store.https_request("whsec", "GET", "/v1/charges")


@tagged("post_install", "-at_install")
class TestSecretOperations(BaseCase):
    def test_hmac_sign_and_verify(self):
        # Tests [@ANCHOR: zero_sudo:secret_operations_hmac_sign]
        # Tests [@ANCHOR: zero_sudo:secret_operations_hmac_verify]
        sig = ops.hmac_sign("k", "p")
        self.assertEqual(len(sig), 64)
        self.assertEqual(len(ops.hmac_sign("k", "p", "sha512")), 128)
        with self.assertRaises(ValueError):
            ops.hmac_sign("k", "p", "md5")
        with self.assertRaises(ValueError):
            ops.hmac_sign("k", b"p")
        self.assertTrue(ops.hmac_verify("k", "p", sig.upper()))
        self.assertFalse(ops.hmac_verify("k", "p", None))
        self.assertFalse(ops.hmac_verify("other", "p", sig))

    def test_stripe_webhook_verify(self):
        # Tests [@ANCHOR: zero_sudo:secret_operations_stripe_webhook_verify]
        now = 1_700_000_000
        good = ops.hmac_sign(SECRET, f"{now}.body")
        self.assertTrue(ops.stripe_webhook_verify(SECRET, "body", f"t={now},v1=00,v1={good}", now=now + 299))
        cases = [
            ("body", f"t={now},v1={good}", 300, now + 301),       # too old
            ("body", f"t={now},v1={good}", 300, now - 301),       # from the future
            ("body2", f"t={now},v1={good}", 300, now),            # other payload
            ("body", f"t={now}", 300, now),                       # no v1
            ("body", f"v1={good}", 300, now),                     # no t
            ("body", f"t=abc,v1={good}", 300, now),               # t not an integer
            ("body", f"t={now},v1={good}", 0, now),               # tolerance switched off
            ("body", None, 300, now),
        ]
        for payload, header, tolerance, at in cases:
            self.assertFalse(ops.stripe_webhook_verify(SECRET, payload, header, tolerance, at), msg=header)


@tagged("post_install", "-at_install")
class TestSecretStoreConfiguration(BaseCase):
    def setUp(self):
        super().setUp()
        self.tmp = tempfile.mkdtemp(prefix="secret_store_")
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def test_broker_client_and_store_routing(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_broker_client_init]
        # Tests [@ANCHOR: zero_sudo:secret_store_broker_client_call]
        def reply(request):
            if request["op"] == "refuse":
                return {"ok": False, "error": "unknown_operation"}
            return {"ok": True, "result": f"{request['op']}:{request['args'].get('name')}"}

        broker = _FakeBroker(self.tmp, reply)
        self.addCleanup(broker.close)
        client = ss.BrokerClient(broker.path)
        self.assertEqual(client.call("hmac_sign", name="a", payload="p"), "hmac_sign:a")
        with self.assertRaises(ss.SecretStoreError) as caught:
            client.call("refuse")
        self.assertIn("unknown_operation", str(caught.exception))
        # With a broker every operation goes there, and require() does not need a local secret.
        store = ss.SecretStore([], broker=client)
        store.require("anything")
        self.assertEqual(store.hmac_sign("a", "p"), "hmac_sign:a")
        self.assertEqual(store.hmac_verify("a", "p", "00"), "hmac_verify:a")
        self.assertEqual(store.stripe_webhook_verify("a", "p", "t=1"), "stripe_webhook_verify:a")
        self.assertEqual(store.https_request("a", "GET", "/v1/x"), "https_request:a")
        self.assertEqual(broker.requests[-1]["args"], {"name": "a", "method": "GET", "path": "/v1/x",
                                                       "body": None, "headers": {}})

    def test_store_from_spec(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_store_from_spec]
        env = {"CREDENTIALS_DIRECTORY": self.tmp}
        store = ss.store_from_spec("systemd, file:/etc/x, encrypted:/k:/v.json, broker:/run/b.sock", env)
        self.assertEqual([b.label for b in store.backends], ["systemd", "file", "encrypted"])
        self.assertEqual(store.broker.socket_path, "/run/b.sock")
        self.assertEqual(ss.store_from_spec("", env).backends, [])
        for bad in ("systemd", "file", "encrypted:/only-one", "plaintext:/x", "broker"):
            with self.assertRaises(ss.SecretStoreError, msg=bad):
                ss.store_from_spec(bad, {})

    def test_default_spec_and_store(self):
        # Tests [@ANCHOR: zero_sudo:secret_store_default_spec]
        # Tests [@ANCHOR: zero_sudo:secret_store_default_store]
        everything = ss.default_spec({"CREDENTIALS_DIRECTORY": self.tmp}, exists=lambda p: True)
        self.assertEqual(
            everything,
            f"broker:{ss.DEFAULT_BROKER_SOCKET},systemd,file:{ss.DEFAULT_CREDENTIALS_FILE}",
        )
        self.assertEqual(ss.default_spec({}, exists=lambda p: False), "")
        store = ss.default_store({"HAMS_SECRET_STORE": "systemd", "CREDENTIALS_DIRECTORY": self.tmp})
        self.assertEqual([b.label for b in store.backends], ["systemd"])
        self.assertIsNone(store.broker)
