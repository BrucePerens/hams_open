# SPDX-License-Identifier: AGPL-3.0-or-later
"""
The pager monitor's RabbitMQ check (generalized_monitor.amqp_check).

It used to open a socket to port 5672, send the 8-byte AMQP protocol header, read the reply and drop the socket, which
the broker logged as an abruptly closed connection with no handshake, once a minute (about 1,440 a day). It now
completes a real AMQP connection: authenticate, open the vhost, open a channel, close the channel and the connection
with their own handshakes. These tests run it against a fake AMQP server that records every method the client sends,
for the success path and for each failure class, and against the real local broker.
"""
import inspect
import os
import socket
import threading
import time
from unittest.mock import patch

import pika
from pika import frame as pika_frame
from pika import spec

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.hams_rabbitmq.models.rabbitmq_pool import resolve_rabbitmq_credentials
import odoo.addons.pager_duty.daemon.generalized_monitor as generalized_monitor

PROTOCOL_HEADER = b"AMQP\x00\x00\x09\x01"
# The fake server runs in the test process itself: parent and child share one loopback whatever the container layout.
LOOPBACK = "127.0.0.1"  # burn-ignore-self-hosted-server


class FakeAmqpServer:
    """A one-connection-at-a-time AMQP 0-9-1 server that records the methods the client sends, in order.

    mode: "ok" (accept everything), "auth_fail" (drop the socket after Start-Ok, as RabbitMQ does for a bad password),
    "vhost_denied" (Connection.Close 530 in answer to Connection.Open), "hang" (accept, never answer),
    "garbage" (answer the header with HTTP), "silent_after_open" (finish the handshake, never answer Channel.Open),
    "header_only" (answer the header with a protocol header and nothing else, the old check's whole conversation).
    """

    def __init__(self, mode="ok"):
        self.mode = mode
        self.methods = []
        self.closed_cleanly = threading.Event()
        self.connections = 0
        self._stop = threading.Event()
        self._socket = socket.socket()
        self._socket.bind((LOOPBACK, 0))
        self._socket.listen(5)
        self._socket.settimeout(0.2)
        self.port = self._socket.getsockname()[1]
        self._thread = threading.Thread(target=self._serve, daemon=True)  # burn-ignore-test-daemon-thread
        self._thread.start()

    def stop(self):
        self._stop.set()
        self._thread.join(5)
        self._socket.close()

    def _serve(self):
        while not self._stop.is_set():
            try:
                conn, _ = self._socket.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            self.connections += 1
            try:
                self._handle(conn)
            except OSError:
                self.methods.append("socket-error")
            finally:
                conn.close()

    def _send(self, conn, channel, method):
        conn.sendall(pika_frame.Method(channel, method).marshal())

    def _next_method(self, conn, buf):
        """Read frames until a method frame is complete. Heartbeats are skipped. Returns (frame, rest)."""
        while True:
            if buf:
                used, frm = pika_frame.decode_frame(buf)  # (0, None) while the frame is incomplete
                if used:
                    buf = buf[used:]
                    if isinstance(frm, pika_frame.Method):
                        self.methods.append(type(frm.method).__name__)
                        return frm, buf
                    continue
            conn.settimeout(10)
            chunk = conn.recv(4096)
            if not chunk:
                return None, buf
            buf += chunk

    def _handle(self, conn):
        conn.settimeout(10)
        header = b""
        while len(header) < 8:
            chunk = conn.recv(8 - len(header))
            if not chunk:
                self.methods.append("closed-before-header")
                return
            header += chunk
        if header != PROTOCOL_HEADER:
            self.methods.append("bad-header")
            return
        if self.mode == "hang":
            self._stop.wait(15)
            return
        if self.mode == "garbage":
            conn.sendall(b"HTTP/1.1 400 Bad Request\r\n\r\n")
            return
        if self.mode == "header_only":
            conn.sendall(PROTOCOL_HEADER)
            time.sleep(0.3)
            return
        self._send(conn, 0, spec.Connection.Start(0, 9, {}, "PLAIN", "en_US"))
        buf = b""
        frm, buf = self._next_method(conn, buf)  # Connection.StartOk
        if frm is None or self.mode == "auth_fail":
            return
        self._send(conn, 0, spec.Connection.Tune(2047, 131072, 0))
        frm, buf = self._next_method(conn, buf)  # Connection.TuneOk
        frm, buf = self._next_method(conn, buf)  # Connection.Open
        if frm is None:
            return
        if self.mode == "vhost_denied":
            self._send(conn, 0, spec.Connection.Close(530, "NOT_ALLOWED - access to vhost refused", 0, 0))
            self._next_method(conn, buf)
            return
        self._send(conn, 0, spec.Connection.OpenOk())
        frm, buf = self._next_method(conn, buf)  # Channel.Open
        if frm is None or self.mode == "silent_after_open":
            if self.mode == "silent_after_open":
                self._stop.wait(15)
            return
        self._send(conn, frm.channel_number, spec.Channel.OpenOk())
        while True:
            frm, buf = self._next_method(conn, buf)
            if frm is None:
                return
            if isinstance(frm.method, spec.Channel.Close):
                self._send(conn, frm.channel_number, spec.Channel.CloseOk())
            elif isinstance(frm.method, spec.Connection.Close):
                self._send(conn, 0, spec.Connection.CloseOk())
                self.closed_cleanly.set()
                return


@tagged("post_install", "-at_install")
class TestAmqpCheck(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.servers = []
        # Keep the tests quick: the stack timeout bounds the "hang" case, the heartbeat the "silent_after_open" case.
        self.safe_patch_object(generalized_monitor, "AMQP_STACK_TIMEOUT", 1.5)
        self.safe_patch_object(generalized_monitor, "AMQP_SOCKET_TIMEOUT", 1)
        self.safe_patch_object(generalized_monitor, "AMQP_HEARTBEAT", 1)

    def _server(self, mode="ok"):
        server = FakeAmqpServer(mode)
        self.servers.append(server)
        self.addCleanup(server.stop)
        return server

    def _check(self, server, user="u", password="p", vhost="/"):
        return generalized_monitor.execute_check(
            {
                "type": "rabbitmq",
                "target": LOOPBACK,
                "port": server.port,
                "user": user,
                "password": password,
                "vhost": vhost,
            }
        )

    def test_01_success_is_a_full_amqp_transaction_closed_cleanly(self):
        # [@ANCHOR: pager_duty:test_amqp_check_full_transaction]
        # Tests [@ANCHOR: pager_duty:amqp_check]
        server = self._server("ok")
        ok, msg = self._check(server)
        self.assertTrue(ok, msg)
        self.assertEqual(msg, "OK")
        self.assertTrue(server.closed_cleanly.wait(3), "the broker must see Connection.Close, not a dropped socket")
        self.assertEqual(
            server.methods,
            [
                "StartOk",
                "TuneOk",
                "Open",
                "Open",  # Channel.Open (the Connection.Open above has the same class name)
                "Close",  # Channel.Close
                "Close",  # Connection.Close
            ],
        )

    def test_02_the_old_header_only_conversation_is_gone(self):
        # [@ANCHOR: pager_duty:test_amqp_check_header_only_gone]
        # A server that answers the header and nothing else must now be a failure: protocol reply alone proves nothing.
        server = self._server("header_only")
        ok, msg = self._check(server)
        self.assertFalse(ok, msg)
        self.assertIn("protocol", msg.lower())
        # And the code no longer contains the raw send of the protocol header in the rabbitmq branch.
        source = inspect.getsource(generalized_monitor.execute_check)
        rabbit_branch = source[source.index('ctype == "rabbitmq"'):source.index('ctype == "xmlrpc"')]
        self.assertNotIn("sendall", rabbit_branch)
        self.assertNotIn("create_connection", rabbit_branch)
        self.assertNotIn("x09\\x01", rabbit_branch)

    def test_03_refused(self):
        s = socket.socket()
        s.bind((LOOPBACK, 0))
        port = s.getsockname()[1]
        s.close()
        ok, msg = generalized_monitor.execute_check(
            {"type": "rabbitmq", "target": LOOPBACK, "port": port, "user": "u", "password": "p"}
        )
        self.assertFalse(ok)
        self.assertIn("refused", msg)
        self.assertIn(f"{LOOPBACK}:{port}", msg)

    def test_04_timeout(self):
        server = self._server("hang")
        started = time.monotonic()
        ok, msg = self._check(server)
        self.assertFalse(ok)
        self.assertIn("timed out", msg)
        self.assertLess(time.monotonic() - started, 6, "a hung broker must not hold the check")

    def test_05_authentication_failure(self):
        server = self._server("auth_fail")
        ok, msg = self._check(server, user="monitor", password="wrong-password-sentinel")
        self.assertFalse(ok)
        self.assertIn("authentication failed", msg)
        self.assertIn("monitor", msg)
        self.assertNotIn("wrong-password-sentinel", msg, "the password must never appear in the check result")

    def test_06_vhost_refused(self):
        server = self._server("vhost_denied")
        ok, msg = self._check(server, vhost="/nope")
        self.assertFalse(ok)
        self.assertIn("RabbitMQ", msg)
        self.assertIn("access denied", msg)

    def test_07_not_amqp(self):
        server = self._server("garbage")
        ok, msg = self._check(server)
        self.assertFalse(ok)
        self.assertIn("protocol error", msg)

    def test_08_silent_after_handshake_does_not_hang(self):
        server = self._server("silent_after_open")
        started = time.monotonic()
        ok, msg = self._check(server)
        self.assertFalse(ok)
        self.assertIn("RabbitMQ", msg)
        self.assertLess(time.monotonic() - started, 20, "a broker that goes silent mid-session must not hold the check")

    def test_09_no_credentials_fails_loudly_and_never_falls_back_to_guest(self):
        server = self._server("ok")
        with patch.dict(os.environ, clear=False):
            os.environ.pop("RMQ_USER", None)
            os.environ.pop("RMQ_PASS", None)
            ok, msg = generalized_monitor.execute_check({"type": "rabbitmq", "target": LOOPBACK, "port": server.port})
        self.assertFalse(ok)
        self.assertIn("no credentials", msg)
        self.assertEqual(server.connections, 0, "without credentials the check must not even open a connection")

    def test_10_no_thread_or_socket_is_left_behind(self):
        # Every failure class leaves no pika thread or open connection behind.
        before = threading.active_count()
        for mode in ("ok", "auth_fail", "garbage", "vhost_denied"):
            self._check(self._server(mode))
        time.sleep(0.2)
        self.assertLessEqual(threading.active_count(), before + len(self.servers), "no pika threads may linger")

    def test_11_real_broker_round_trip(self):
        # Tests [@ANCHOR: pager_duty:amqp_check]
        utils = self.env["zero_sudo.security.utils"]
        pair = resolve_rabbitmq_credentials(
            utils._get_system_param("rabbitmq.user"),
            utils._get_system_param("rabbitmq.pass"),
            os.environ,  # burn-ignore-env: RMQ_USER/RMQ_PASS from rabbitmq.env, as the pool resolves them
        )
        self.assertIsNotNone(pair, "this test needs the real local RabbitMQ and its credentials")
        user, password = pair
        host = os.environ.get("RABBITMQ_HOST", "rabbitmq")  # burn-ignore-env
        port = int(utils._get_system_param("rabbitmq.port") or 5672)
        vhost = utils._get_system_param("rabbitmq.vhost") or "/"
        ok, msg = generalized_monitor.amqp_check(host, port, user, password, vhost)
        self.assertTrue(ok, msg)
        wrong = "not-the-password-" + password[::-1]
        ok, msg = generalized_monitor.amqp_check(host, port, user, wrong, vhost)
        self.assertFalse(ok)
        self.assertIn("authentication failed", msg)
        self.assertNotIn(wrong, msg)
