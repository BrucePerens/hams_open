# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for ham_tls, over real loopback sockets with a recording/tampering relay."""

import os
import socket
import ssl
import subprocess
import tempfile
import threading
import unittest

import ham_tls


def make_cert(directory, name):
    key, cert = os.path.join(directory, name + ".key"), os.path.join(directory, name + ".pem")
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert,
         "-subj", "/CN=" + name, "-addext", "subjectAltName=DNS:" + name, "-days", "2"],
        check=True, capture_output=True)
    return cert, key


class Relay:
    """Loopback relay between client and server; records the client->server bytes and may alter them."""

    def __init__(self, target, mutate=None):
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))  # burn-ignore-test-local-server: hermetic loopback fixture
        self.listener.listen()
        self.address = self.listener.getsockname()
        self.target, self.mutate, self.seen, self.chunks = target, mutate, b"", []
        threading.Thread(target=self._run, daemon=True).start()

    def _pump(self, src, dst, upstream):
        while True:
            data = src.recv(65536)
            if not data:
                break
            if upstream:
                self.seen += data
                self.chunks.append(data)
                data = self.mutate(data, len(self.chunks)) if self.mutate else data
            dst.sendall(data)
        dst.close()

    def _run(self):
        client, _ = self.listener.accept()
        server = socket.create_connection(self.target)
        threading.Thread(target=self._pump, args=(server, client, False), daemon=True).start()
        self._pump(client, server, True)


class Echo:
    def __init__(self, certfile, keyfile, ciphers=None):
        context = ham_tls.server_context(certfile, keyfile)
        if ciphers:
            context.minimum_version = ssl.TLSVersion.MINIMUM_SUPPORTED
            context.maximum_version = ssl.TLSVersion.TLSv1_2
            context.set_ciphers(ciphers)
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))  # burn-ignore-test-local-server: hermetic loopback fixture
        self.listener.listen()
        self.address, self.context, self.received, self.error = self.listener.getsockname(), context, [], None
        threading.Thread(target=self._run, daemon=True).start()

    def _run(self):
        conn, _ = self.listener.accept()
        try:
            tls = self.context.wrap_socket(conn, server_side=True)
            while True:
                data = tls.recv(4096)
                if not data:
                    break
                self.received.append(data)
                tls.sendall(b"ok")
        except (ssl.SSLError, OSError) as error:
            self.error = error


class HamTlsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.cert, cls.key = make_cert(cls.directory.name, "node.ham")
        cls.other_cert, cls.other_key = make_cert(cls.directory.name, "evil.ham")

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def client(self, address, host="node.ham", cafile=None):
        tls = ham_tls.connect(socket.create_connection(address), ham_tls.client_context(cafile or self.cert), host)
        self.addCleanup(tls.close)
        return tls

    def test_payload_is_readable_on_the_wire_with_an_integrity_only_suite(self):
        # Tests [@ANCHOR: ham_tls:connect]
        # Tests [@ANCHOR: ham_tls:server_context]
        server = Echo(self.cert, self.key)
        relay = Relay(server.address)
        tls = self.client(relay.address)
        self.assertEqual(tls.cipher()[0], "NULL-SHA256")
        tls.sendall(b"GET /secretless HTTP/1.1")
        self.assertEqual(tls.recv(2), b"ok")
        self.assertIn(b"GET /secretless HTTP/1.1", relay.seen)

    def test_a_tampered_record_is_detected(self):
        # Tests [@ANCHOR: ham_tls:connect]
        def flip(data, index):
            if index > 1 and b"payload" in data:
                return data.replace(b"payload", b"pbyload")
            return data
        server = Echo(self.cert, self.key)
        relay = Relay(server.address, flip)
        tls = self.client(relay.address)
        tls.sendall(b"payload")
        with self.assertRaises((ssl.SSLError, ConnectionError)):
            tls.recv(2)
        self.assertEqual(server.received, [])

    def test_a_replayed_record_is_detected(self):
        # Tests [@ANCHOR: ham_tls:connect]
        state = {}

        def replay(data, index):
            if b"first" in data:
                state["record"] = data
            elif b"second" in data:
                return state["record"]  # the earlier record again instead of the new one
            return data
        server = Echo(self.cert, self.key)
        relay = Relay(server.address, replay)
        tls = self.client(relay.address)
        tls.sendall(b"first")
        self.assertEqual(tls.recv(2), b"ok")
        tls.sendall(b"second")
        with self.assertRaises((ssl.SSLError, ConnectionError)):
            tls.recv(2)
        self.assertEqual(server.received, [b"first"])

    def test_wrong_certificate_is_refused(self):
        # Tests [@ANCHOR: ham_tls:client_context]
        server = Echo(self.other_cert, self.other_key)
        with self.assertRaises(ssl.SSLCertVerificationError):
            self.client(server.address)

    def test_wrong_hostname_is_refused(self):
        # Tests [@ANCHOR: ham_tls:client_context]
        server = Echo(self.cert, self.key)
        with self.assertRaises(ssl.SSLCertVerificationError):
            self.client(server.address, host="other.ham")

    def test_server_that_only_offers_an_encrypting_suite_is_refused(self):
        # Tests [@ANCHOR: ham_tls:connect]
        server = Echo(self.cert, self.key, ciphers="ECDHE-RSA-AES128-GCM-SHA256")
        with self.assertRaises(ssl.SSLError):
            self.client(server.address)

    def test_suite_check_refuses_an_encrypting_connection(self):
        # Tests [@ANCHOR: ham_tls:require_integrity_only]
        server = Echo(self.cert, self.key, ciphers="ECDHE-RSA-AES128-GCM-SHA256")
        context = ssl.create_default_context(cafile=self.cert)  # a stock context negotiates encryption
        tls = context.wrap_socket(socket.create_connection(server.address), server_hostname="node.ham")
        self.addCleanup(tls.close)
        with self.assertRaises(ham_tls.EncryptedLinkError):
            ham_tls.require_integrity_only(tls)

    def test_rfc9150_suite_names_are_accepted_by_the_policy(self):
        # Tests [@ANCHOR: ham_tls:require_integrity_only]
        class Fake:
            def __init__(self, name):
                self.name = name

            def cipher(self):
                return (self.name, "TLSv1.3", 0)
        for name in ("TLS_SHA256_SHA256", "TLS_SHA384_SHA384"):
            self.assertEqual(ham_tls.require_integrity_only(Fake(name)), name)
        with self.assertRaises(ham_tls.EncryptedLinkError):
            ham_tls.require_integrity_only(Fake("TLS_AES_128_GCM_SHA256"))


if __name__ == "__main__":
    unittest.main()
