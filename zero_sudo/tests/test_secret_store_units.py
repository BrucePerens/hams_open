# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of the secret store (secret_store.py), its operations (secret_operations.py) and the broker's
entry point (secret_broker/main.py), added to kill the mutation survivors of the second mutation pass (2026-11-13):
exact refusal messages, permission bits, size and time limits at their edges, the request the client sends, and the
defaults taken from the environment."""
import base64
import contextlib
import io
import json
import os
import shutil
import socket
import tempfile
import threading
import time

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.tests.common import SafePatchMixin
from odoo.addons.zero_sudo.daemon import secret_operations as ops
from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.daemon.secret_broker import main as broker_main

ME = os.getuid()
NOT_ROOT = ME != 0


class _TempCase(SafePatchMixin, BaseCase):
    def setUp(self):
        super().setUp()
        self.dir = tempfile.mkdtemp(prefix="secret_units_")
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)

    def write(self, name, data, mode=0o640):
        path = os.path.join(self.dir, name)
        if os.path.exists(path):
            os.chmod(path, 0o600)
        with open(path, "wb") as handle:
            handle.write(data if isinstance(data, bytes) else data.encode())
        os.chmod(path, mode)
        return path


@tagged("post_install", "-at_install")
class TestSecretStoreFileChecks(_TempCase):

    def test_check_name_message_names_the_value(self):
        for bad, shown in (("../x", "'../x'"), ("A", "'A'"), (5, "5"), ("a..b", "'a..b'"), ("", "''"), (None, "None")):
            with self.assertRaises(ValueError) as caught:
                ss.check_name(bad)
            self.assertEqual(str(caught.exception), "invalid secret name " + shown)
        self.assertEqual(ss.check_name("a.b-c_d9"), "a.b-c_d9")
        self.assertEqual(ss.check_name("a" * 100), "a" * 100)
        with self.assertRaises(ValueError):
            ss.check_name("a" * 101)

    def test_private_file_refusals_and_messages(self):
        path = self.write("f", "x", 0o600)
        self.assertEqual(ss.check_private_file(path, ME).st_uid, ME)
        for mode in (0o601, 0o602, 0o604, 0o644, 0o666, 0o607):
            os.chmod(path, mode)
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                ss.check_private_file(path, ME)
            self.assertEqual(str(caught.exception), "%s: mode %s gives access to every account" % (path, oct(mode)))
        for mode in (0o620, 0o660, 0o670):
            os.chmod(path, mode)
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                ss.check_private_file(path, ME)
            self.assertEqual(str(caught.exception), "%s: group-writable" % path)
        for mode in (0o400, 0o440, 0o640, 0o750, 0o710):
            os.chmod(path, mode)
            ss.check_private_file(path, ME)

    def test_private_file_must_be_a_regular_file_not_a_link_or_directory(self):
        folder = os.path.join(self.dir, "d")
        os.mkdir(folder, 0o700)
        target = self.write("t", "x", 0o600)
        link = os.path.join(self.dir, "l")
        os.symlink(target, link)
        for path in (folder, link):
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                ss.check_private_file(path, ME)
            self.assertEqual(str(caught.exception), "%s: not a regular file" % path)

    def test_private_file_owner_default_is_root(self):
        path = self.write("f", "x", 0o600)
        with self.assertRaises(ss.InsecureSecretSource) as caught:
            ss.check_private_file(path, ME + 1)
        self.assertEqual(str(caught.exception), "%s: owner uid %d, expected %d" % (path, ME, ME + 1))
        if NOT_ROOT:
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                ss.check_private_file(path)
            self.assertEqual(str(caught.exception), "%s: owner uid %d, expected 0" % (path, ME))
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                ss.CredentialsFileBackend(path).get("a")
            self.assertIn("expected 0", str(caught.exception))
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                ss.key_from_file(path)()
            self.assertIn("expected 0", str(caught.exception))

    def test_read_small_file_limit_and_open_flags(self):
        edge = self.write("edge", b"x" * ss.MAX_SECRET_BYTES)
        self.assertEqual(len(ss.read_small_file(edge)), ss.MAX_SECRET_BYTES)
        over = self.write("over", b"x" * (ss.MAX_SECRET_BYTES + 1))
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.read_small_file(over)
        self.assertEqual(str(caught.exception), "%s: larger than 65536 bytes" % over)
        way_over = self.write("way", b"x" * (ss.MAX_SECRET_BYTES * 2))
        with self.assertRaises(ss.SecretStoreError):
            ss.read_small_file(way_over)
        flags = []
        real_open = os.open
        self.safe_patch_object(ss.os, "open", new=lambda path, flag, *a: flags.append(flag) or real_open(path, flag, *a))
        ss.read_small_file(edge)
        self.assertTrue(flags[0] & os.O_NOFOLLOW)
        self.assertTrue(flags[0] & os.O_CLOEXEC)
        link = os.path.join(self.dir, "link")
        os.symlink(edge, link)
        with self.assertRaises(OSError):
            ss.read_small_file(link)

    def test_credentials_text_errors_name_the_line_number(self):
        self.assertEqual(ss.parse_credentials_text("# c\n\n a.b = v \nc=d=e"), {"a.b": "v", "c": "d=e"})
        for text, message in (("bad line", "credentials line 1: expected name=value"),
                              ("# c\n\nBad=1", "credentials line 3: expected name=value"),
                              ("a=1\nnoeq", "credentials line 2: expected name=value"),
                              ("a=1\nb=2\na=3", "credentials line 3: duplicate name 'a'")):
            with self.assertRaises(ss.SecretStoreError) as caught:
                ss.parse_credentials_text(text)
            self.assertEqual(str(caught.exception), message)


@tagged("post_install", "-at_install")
class TestSecretStoreBackends(_TempCase):

    def test_systemd_backend_refuses_world_bits_and_non_files(self):
        backend = ss.SystemdCredentialBackend(self.dir)
        for mode in (0o001, 0o002, 0o004, 0o644):
            path = self.write("cred", "v", mode)
            with self.assertRaises(ss.InsecureSecretSource) as caught:
                backend.get("cred")
            self.assertEqual(str(caught.exception), "%s: not a private regular file" % path)
        os.mkdir(os.path.join(self.dir, "folder"), 0o700)
        with self.assertRaises(ss.InsecureSecretSource):
            backend.get("folder")
        self.write("ok", " value \n", 0o440)
        self.assertEqual(backend.get("ok"), "value")
        self.assertIsNone(backend.get("missing"))

    def test_encrypt_requires_exactly_32_bytes(self):
        for size in (0, 16, 24, 31, 33):
            with self.assertRaises(ValueError) as caught:
                ss.encrypt_value(b"k" * size, "n", "v")
            self.assertEqual(str(caught.exception), "the key must be 32 bytes")
        token = ss.encrypt_value(b"k" * 32, "n", "v")
        self.assertTrue(token.startswith("hs1."))
        self.assertEqual(ss.decrypt_value(b"k" * 32, "n", token), "v")

    def test_decrypt_messages(self):
        key = b"k" * 32
        token = ss.encrypt_value(key, "n", "v")
        wrong_format = "XX1." + token.partition(".")[2]
        for args in ((key, "n", wrong_format), (b"k" * 16, "n", token), (key, "n", "nodots")):
            with self.assertRaises(ss.SecretStoreError) as caught:
                ss.decrypt_value(*args)
            self.assertEqual(str(caught.exception), "secret 'n': not a hs1 token or wrong key size")
        for args in ((b"j" * 32, "n", token), (key, "other", token), (key, "n", "hs1.!!!"), (key, "n", "hs1.")):
            with self.assertRaises(ss.SecretStoreError) as caught:
                ss.decrypt_value(*args)
            self.assertEqual(str(caught.exception), "secret '%s': does not decrypt with this key and name" % args[1])

    def test_key_file_and_json_lookup_messages(self):
        short = self.write("short", base64.urlsafe_b64encode(b"k" * 16), 0o600)
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.key_from_file(short, ME)()
        self.assertEqual(str(caught.exception), "%s: the key must decode to 32 bytes" % short)
        good = self.write("good", base64.urlsafe_b64encode(b"k" * 32) + b"\n", 0o600)
        self.assertEqual(ss.key_from_file(good, ME)(), b"k" * 32)
        listing = self.write("list.json", "[1]")
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.json_file_lookup(listing)("n")
        self.assertEqual(str(caught.exception), "%s: expected a JSON object" % listing)
        self.assertIsNone(ss.json_file_lookup(os.path.join(self.dir, "none.json"))("n"))
        self.assertEqual(ss.json_file_lookup(self.write("o.json", '{"n": "t"}'))("n"), "t")


class _OneShotBroker:
    """A unix socket server: reads the whole request (to EOF), records it, answers with `reply` bytes."""

    count = 0

    def __init__(self, directory, reply):
        _OneShotBroker.count += 1
        self.path = os.path.join(directory, "b%d.sock" % _OneShotBroker.count)
        self.reply = reply
        self.requests = []
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(self.path)
        self.server.listen(2)
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
                while True:
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                self.requests.append(data)
                conn.sendall(self.reply)

    def close(self):
        self.server.close()


@tagged("post_install", "-at_install")
class TestBrokerClientAndStore(_TempCase):

    def _broker(self, reply):
        broker = _OneShotBroker(self.dir, reply)
        self.addCleanup(broker.close)
        return broker

    def test_the_request_is_one_exact_json_line_and_the_broker_sees_end_of_input(self):
        broker = self._broker(b'{"ok": true, "result": 7}')
        client = ss.BrokerClient(broker.path, timeout=5.0)
        self.assertEqual(client.call("hmac_sign", name="n", payload="p"), 7)
        self.assertEqual(broker.requests, [b'{"v": 1, "op": "hmac_sign", "args": {"name": "n", "payload": "p"}}\n'])

    def test_a_reply_larger_than_one_read_is_joined(self):
        broker = self._broker(json.dumps({"ok": True, "result": "x" * 200000}).encode())
        self.assertEqual(ss.BrokerClient(broker.path, timeout=5.0).call("op"), "x" * 200000)

    def test_refusals_carry_the_broker_error_code(self):
        broker = self._broker(b'{"ok": false, "error": "denied"}')
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.BrokerClient(broker.path, timeout=5.0).call("hmac_sign")
        self.assertEqual(str(caught.exception), "broker refused hmac_sign: denied")
        broker = self._broker(b'{"ok": false}')
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.BrokerClient(broker.path, timeout=5.0).call("hmac_sign")
        self.assertEqual(str(caught.exception), "broker refused hmac_sign: unknown")
        broker = self._broker(b'{"ok": "yes", "result": 1}')
        with self.assertRaises(ss.SecretStoreError):
            ss.BrokerClient(broker.path, timeout=5.0).call("x")

    def test_the_client_gives_up_after_its_timeout(self):
        path = os.path.join(self.dir, "silent.sock")
        silent = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        silent.bind(path)
        silent.listen(1)
        self.addCleanup(silent.close)
        started = time.monotonic()
        with self.assertRaises(OSError):
            ss.BrokerClient(path, timeout=0.3).call("op")
        self.assertLess(time.monotonic() - started, 5)
        self.assertEqual(ss.BrokerClient(path).timeout, 10.0)

    def test_get_names_the_backends_it_tried(self):
        class Backend:
            def __init__(self, label, value=None):
                self.label, self.value = label, value

            def get(self, name):
                return self.value
        with self.assertRaises(ss.SecretNotFound) as caught:
            ss.SecretStore([Backend("systemd"), Backend("file")]).get("a")
        self.assertEqual(str(caught.exception), "secret 'a' is not in the secret store (systemd, file)")
        with self.assertRaises(ss.SecretNotFound) as caught:
            ss.SecretStore().get("a")
        self.assertEqual(str(caught.exception), "secret 'a' is not in the secret store (no Tier A backend configured)")
        self.assertEqual(ss.SecretStore([Backend("a"), Backend("b", "found")]).get("a"), "found")
        self.assertEqual(ss.SecretStore([Backend("a", ""), Backend("b", "later")]).get("a"), "")  # an empty value is a value

    def test_stripe_default_tolerance_is_300_seconds_and_the_broker_is_told_so(self):
        secret, payload, now = "whsec_unit", "{}", 1700000000
        store = ss.SecretStore([type("B", (), {"label": "x", "get": lambda self, name: secret})()])
        def header(t):
            return "t=%d,v1=%s" % (t, ops.hmac_sign(secret, "%d.%s" % (t, payload)))

        self.assertTrue(store.stripe_webhook_verify("n", payload, header(now - 300), now=now))
        self.assertFalse(store.stripe_webhook_verify("n", payload, header(now - 301), now=now))
        self.assertTrue(store.stripe_webhook_verify("n", payload, header(now - 301), tolerance=301, now=now))
        sent = []

        class Broker:
            def call(self, op, **args):
                sent.append((op, args))
                return True
        with_broker = ss.SecretStore([], Broker())
        self.assertTrue(with_broker.stripe_webhook_verify("n", payload, "h"))
        self.assertEqual(sent, [("stripe_webhook_verify", {"name": "n", "payload": payload, "signature_header": "h", "tolerance": 300})])

    def test_https_request_needs_the_broker(self):
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.SecretStore().https_request("n", "GET", "/p")
        self.assertEqual(str(caught.exception), "https_request needs the Tier B broker (secret_broker.py)")


@tagged("post_install", "-at_install")
class TestStoreFromSpec(_TempCase):

    def _environment(self, **values):
        saved = {key: os.environ.get(key) for key in values}

        def restore():
            for key, value in saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
        self.addCleanup(restore)
        for key, value in values.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    def test_unknown_entries_are_refused_with_the_entry_in_the_message(self):
        for spec in ("file", "systemd:x", "nonsense", "encrypted:onlyone", "encrypted:a:b:c", "broker", "file:"):
            with self.assertRaises(ss.SecretStoreError) as caught:
                ss.store_from_spec(spec, {"CREDENTIALS_DIRECTORY": "/d"})
            self.assertEqual(str(caught.exception), "unknown secret store entry %r" % spec)
        with self.assertRaises(ss.SecretStoreError) as caught:
            ss.store_from_spec("systemd", {})
        self.assertEqual(str(caught.exception), "secret store 'systemd' needs $CREDENTIALS_DIRECTORY (LoadCredential=)")
        with self.assertRaises(ss.SecretStoreError):
            ss.store_from_spec("systemd", {"CREDENTIALS_DIRECTORY": ""})

    def test_each_entry_kind_builds_its_backend_in_order(self):
        store = ss.store_from_spec(" systemd , file:/f ,encrypted:/k:/j,broker:/s.sock,, ", {"CREDENTIALS_DIRECTORY": "/d"})
        self.assertEqual([b.label for b in store.backends], ["systemd", "file", "encrypted"])
        self.assertEqual(store.backends[0].directory, "/d")
        self.assertEqual(store.backends[1].path, "/f")
        self.assertEqual(store.broker.socket_path, "/s.sock")
        empty = ss.store_from_spec("", {})
        self.assertEqual((empty.backends, empty.broker), ([], None))

    def test_the_process_environment_is_the_default(self):
        self._environment(CREDENTIALS_DIRECTORY="/cred")
        self.assertEqual(ss.store_from_spec("systemd").backends[0].directory, "/cred")
        self.assertIn("systemd", ss.default_spec(exists=lambda path: False))
        self._environment(CREDENTIALS_DIRECTORY=None)
        self.assertEqual(ss.default_spec(exists=lambda path: False), "")

    def test_default_spec_order(self):
        def both(path):
            return True
        self.assertEqual(ss.default_spec({"CREDENTIALS_DIRECTORY": "/d"}, exists=both),
                         "broker:%s,systemd,file:%s" % (ss.DEFAULT_BROKER_SOCKET, ss.DEFAULT_CREDENTIALS_FILE))
        self.assertEqual(ss.default_spec({}, exists=lambda path: path == ss.DEFAULT_CREDENTIALS_FILE),
                         "file:%s" % ss.DEFAULT_CREDENTIALS_FILE)
        self.assertEqual(ss.default_spec({"CREDENTIALS_DIRECTORY": ""}, exists=lambda path: False), "")

    def test_default_store_reads_the_spec_from_the_environment(self):
        store = ss.default_store({"HAMS_SECRET_STORE": "file:/f", "CREDENTIALS_DIRECTORY": "/d"})
        self.assertEqual([b.label for b in store.backends], ["file"])
        explicit_empty = ss.default_store({"HAMS_SECRET_STORE": "", "CREDENTIALS_DIRECTORY": "/d"})  # set but empty is a choice
        self.assertEqual(explicit_empty.backends, [])
        self.assertEqual([b.label for b in ss.default_store({"CREDENTIALS_DIRECTORY": "/d"}).backends][:1], ["systemd"])
        self._environment(HAMS_SECRET_STORE="file:/from-process")
        self.assertEqual(ss.default_store().backends[0].path, "/from-process")


@tagged("post_install", "-at_install")
class TestSecretOperationsEdges(BaseCase):

    SECRET, PAYLOAD = "whsec_ops", '{"a":1}'

    def header(self, t, *extra, key="v1", payload=None):
        return ",".join(["t=%s" % t, "%s=%s" % (key, ops.hmac_sign(self.SECRET, "%s.%s" % (t, payload or self.PAYLOAD)))] + list(extra))

    def verify(self, header, **kwargs):
        return ops.stripe_webhook_verify(self.SECRET, self.PAYLOAD, header, **kwargs)

    def test_error_messages(self):
        with self.assertRaises(ValueError) as caught:
            ops.hmac_sign("s", "p", "md5")
        self.assertEqual(str(caught.exception), "unsupported digest 'md5'")
        with self.assertRaises(ValueError) as caught:
            ops.hmac_sign("s", b"p")
        self.assertEqual(str(caught.exception), "payload must be a string")

    def test_verify_returns_exactly_false_for_junk_signatures(self):
        good = ops.hmac_sign("s", "p")
        self.assertIs(ops.hmac_verify("s", "p", good), True)
        self.assertIs(ops.hmac_verify("s", "p", good.upper()), True)
        for junk in (None, 5, b"x", "\ud800", "é" * 64, "", good[:-1]):
            self.assertIs(ops.hmac_verify("s", "p", junk), False, repr(junk))

    def test_stripe_tolerance_default_and_edges(self):
        now = 1700000000
        self.assertIs(self.verify(self.header(now - 300), now=now), True)
        self.assertIs(self.verify(self.header(now + 300), now=now), True)
        self.assertIs(self.verify(self.header(now - 301), now=now), False)
        self.assertIs(self.verify(self.header(now + 301), now=now), False)
        self.assertIs(self.verify(self.header(now - 1), tolerance=1, now=now), True)
        self.assertIs(self.verify(self.header(now - 2), tolerance=1, now=now), False)
        for tolerance in (0, -1, 1.5, "300", None):
            self.assertIs(self.verify(self.header(now), tolerance=tolerance, now=now), False, repr(tolerance))

    def test_stripe_uses_the_clock_when_no_time_is_given(self):
        self.assertIs(self.verify(self.header(int(time.time()))), True)
        self.assertIs(self.verify(self.header(int(time.time()) - 3600)), False)

    def test_stripe_timestamp_shape(self):
        twelve = "999999999999"
        self.assertIs(self.verify(self.header(twelve), tolerance=10, now=int(twelve)), True)
        thirteen = "1" + twelve
        self.assertIs(self.verify(self.header(thirteen), tolerance=10, now=int(thirteen)), False)
        arabic = "\u0661\u0662\u0663"
        self.assertIs(self.verify(self.header(arabic), tolerance=10, now=123), False)
        for t in ("", "abc", "-5", "1.5", " "):
            self.assertIs(self.verify(self.header(t), now=1), False, repr(t))

    def test_stripe_header_and_candidates(self):
        now = 1700000000
        valid = self.header(now)
        self.assertIs(self.verify(self.header(now, key="v0"), now=now), False)  # only v1 counts
        self.assertIs(self.verify(self.header(now, key="x1"), now=now), False)
        self.assertIs(self.verify("t=%d,v1=%s" % (now, "0" * 64) + "," + valid.split(",", 1)[1], now=now), True)  # any v1 may match
        self.assertIs(self.verify("t=%d" % now, now=now), False)
        self.assertIs(self.verify("v1=%s" % ops.hmac_sign(self.SECRET, "1.%s" % self.PAYLOAD), now=1), False)  # no t
        self.assertIs(self.verify("t=%d,v1=\ud800" % now, now=now), False)
        for header in (None, 5, b"t=1"):
            self.assertIs(self.verify(header, now=now), False)
        self.assertIs(ops.stripe_webhook_verify(self.SECRET, None, valid, now=now), False)
        self.assertIs(ops.stripe_webhook_verify(self.SECRET, b"x", valid, now=now), False)


@tagged("post_install", "-at_install")
class TestBrokerMain(_TempCase):

    def test_build_refuses_root_and_names_the_credentials_unit_setting(self):
        with self.assertRaises(broker_main.broker.BrokerConfigError) as caught:
            broker_main.build({}, {}, 0)
        self.assertEqual(str(caught.exception), "the broker must not run as root; run it as its own account")

    def test_build_validates_the_config_before_anything_else_and_checks_the_unit(self):
        seen = []
        self.safe_patch_object(broker_main.broker, "validate_config", new=lambda config: seen.append(config) or None)
        config = {"allowed_users": [], "credentials": "systemd"}
        with self.assertRaises(broker_main.broker.BrokerConfigError) as caught:
            broker_main.build(config, {}, 1000)
        self.assertEqual(seen, [config])
        self.assertEqual(str(caught.exception), "credentials 'systemd' needs LoadCredential= in the unit")
        with self.assertRaises(broker_main.broker.BrokerConfigError):
            broker_main.build(config, {"CREDENTIALS_DIRECTORY": ""}, 1000)
        with self.assertRaises(KeyError):
            broker_main.build({"credentials": "x"}, {}, 1000)  # validate_config is stubbed; the missing key proves it ran first

    def _run(self, argv, build, config_text=None):
        calls = []
        self.safe_patch_object(broker_main, "build", new=lambda config, environ, euid: calls.append((config, environ, euid)) or build)
        logged = []
        self.safe_patch_object(broker_main.logging, "basicConfig", new=lambda **kwargs: logged.append(kwargs))
        return calls, logged

    def test_main_serves_and_exits_zero(self):
        served = []
        instance = type("I", (), {"serve": lambda self: served.append(1)})()
        path = self.write_config('{"k": 1}')
        calls, logged = self._run(["--config", path], instance)
        self.assertEqual(broker_main.main(["--config", path]), 0)
        self.assertEqual(served, [1])
        self.assertEqual(calls, [({"k": 1}, os.environ, os.geteuid())])
        self.assertEqual(logged, [{"level": broker_main.logging.INFO, "format": "%(levelname)s %(message)s"}])

    def write_config(self, text):
        path = os.path.join(self.dir, "broker.json")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
        return path

    def test_main_refuses_to_start_with_status_two_and_logs_why(self):
        calls, logged = self._run([], None)
        missing = os.path.join(self.dir, "missing.json")
        with self.assertLogs(level="ERROR") as logs:
            self.assertEqual(broker_main.main(["--config", missing]), 2)
        self.assertIn("secret broker refuses to start: ", logs.output[0])
        self.assertIn("missing.json", logs.output[0])
        bad = self.write_config("{not json")
        with self.assertLogs(level="ERROR") as logs:
            self.assertEqual(broker_main.main(["--config", bad]), 2)
        self.assertEqual(calls, [])

        def refuse(config, environ, euid):
            raise broker_main.secret_store.SecretStoreError("no secret")
        self.safe_patch_object(broker_main, "build", new=refuse)
        good = self.write_config("{}")
        with self.assertLogs(level="ERROR") as logs:
            self.assertEqual(broker_main.main(["--config", good]), 2)
        self.assertEqual(logs.output, ["ERROR:root:secret broker refuses to start: no secret"])

    def test_main_reads_the_config_as_utf8_and_defaults_to_the_etc_path(self):
        path = self.write_config('{"name": "caf\u00e9"}')
        calls, logged = self._run([], type("I", (), {"serve": lambda self: None})())
        self.assertEqual(broker_main.main(["--config", path]), 0)
        self.assertEqual(calls[0][0], {"name": "caf\u00e9"})
        opened = []
        real_open = open
        self.safe_patch_object(broker_main, "open", new=lambda p, *a, **k: opened.append(p) or real_open(os.devnull), create=True)
        with self.assertLogs(level="ERROR"):
            broker_main.main([])
        self.assertEqual(opened, ["/etc/hams_secret_broker/broker.json"])

    def test_the_help_text_describes_the_broker(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), self.assertRaises(SystemExit):
            broker_main.main(["--help"])
        self.assertIn("\nTier B secret broker\n", out.getvalue())
