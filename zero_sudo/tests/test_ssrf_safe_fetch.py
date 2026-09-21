# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Real tests for `zero_sudo/daemon/ssrf_safe_fetch.py` -- the shared, tested home for
the SSRF-safety check + pinned-connection fetch mechanism that closes a real
DNS-rebinding TOCTOU previously present in both `binary_downloader/models/
binary_utils.py` and `pager_duty/daemon/pager_synthetic_spooler.py` (see
`ssrf_safe_fetch.py`'s own module docstring for the full writeup, and
`docs/bug_hunt_claims/hams_open/binary_downloader/claims/
binary_utils_is_ssrf_safe_public_ip.md` in hams_com for the original finding).

Deliberately keeps two layers separate, per the module's own design:
- the resolve+validate layer (`resolve_ssrf_safe_addresses`) -- can be fully tested
  with mocked `socket.getaddrinfo`, no real network access.
- the pin layer (`_pinned_create_connection`, `urlopen_ssrf_safe`) -- needs a REAL
  local TCP/TLS server to prove the Host header, TLS SNI, and certificate hostname
  verification all still work correctly; a real loopback-bound test server stands in
  for "the validated target" here (loopback addresses are exactly what
  `resolve_ssrf_safe_addresses`/`is_ssrf_safe_public_ip` must themselves reject --
  already covered by the tests that exercise the classification layer directly, not
  by these tests, which mock `socket.getaddrinfo` to point straight at the loopback
  test server, isolating "does the connection follow the validated address" from
  "is loopback correctly rejected").
"""

import http.server
import ipaddress
import os
import socket
import ssl
import subprocess
import shutil
import tempfile
import threading
import urllib.error
import urllib.request

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.daemon import ssrf_safe_fetch as sf


def _generate_self_signed_cert(cert_path, key_path, common_name):
    """Shells out to the real `openssl` binary to generate a real self-signed
    certificate + key for `common_name`, the same way `HamsHttpCase.setUpClass`
    (zero_sudo/tests/common.py) already provisions its own real HTTPS test
    certificate -- a real cert exercising real TLS handshake/verification code,
    not a mocked ssl.SSLContext."""
    subprocess.run(
        [
            "openssl", "req", "-x509", "-newkey", "rsa:2048",
            "-keyout", key_path, "-out", cert_path,
            "-days", "1", "-nodes",
            "-subj", f"/CN={common_name}",
            "-addext", f"subjectAltName=DNS:{common_name}",
        ],
        check=True,
        capture_output=True,
    )


class _RecordingHandler(http.server.BaseHTTPRequestHandler):
    """Records the Host header it actually received and serves a fixed body,
    so a test can assert the real connection carried the ORIGINAL hostname
    (not the pinned IP) in its Host header."""

    recorded_host_headers = []

    def do_GET(self):
        _RecordingHandler.recorded_host_headers.append(self.headers.get("Host"))
        body = b"ssrf-safe-fetch-ok"
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):  # silence BaseHTTPRequestHandler's own stderr logging
        pass


@tagged("post_install", "-at_install")
class TestSsrfSafeFetch(HamsTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls._cert_dir = tempfile.mkdtemp(prefix="ssrf_safe_fetch_test_certs_")
        cls.addClassCleanup(shutil.rmtree, cls._cert_dir, ignore_errors=True)
        cls.cert_path = os.path.join(cls._cert_dir, "cert.pem")
        cls.key_path = os.path.join(cls._cert_dir, "key.pem")
        cls.test_hostname = "ssrf-safe-fetch-test.invalid"
        _generate_self_signed_cert(cls.cert_path, cls.key_path, cls.test_hostname)

        cls.recorded_sni = []
        server_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server_ctx.load_cert_chain(cls.cert_path, cls.key_path)
        server_ctx.sni_callback = lambda sslsock, server_name, ssl_ctx: cls.recorded_sni.append(server_name)

        cls.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _RecordingHandler)  # burn-ignore-self-hosted-server: this test spawns its own local server and connects to it in the same process
        cls.httpd.socket = server_ctx.wrap_socket(cls.httpd.socket, server_side=True)
        cls.server_port = cls.httpd.server_address[1]
        cls._server_thread = threading.Thread(target=cls.httpd.serve_forever, daemon=True)  # burn-ignore-test-daemon-thread: explicitly shut down + joined with a timeout in tearDownClass below
        cls._server_thread.start()

        cls.client_ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        cls.client_ssl_context.load_verify_locations(cls.cert_path)
        cls.client_ssl_context.check_hostname = True
        cls.client_ssl_context.verify_mode = ssl.CERT_REQUIRED

        cls._validated_addrinfo_for_test_server = [
            (socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP, "", ("127.0.0.1", cls.server_port))  # burn-ignore-self-hosted-server: pins to the local test server started above, in this same process
        ]

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()
        cls._server_thread.join(timeout=5.0)
        super().tearDownClass()

    def setUp(self):
        super().setUp()
        _RecordingHandler.recorded_host_headers = []
        self.__class__.recorded_sni = []

    # ------------------------------------------------------------------
    # Layer 1: is_ssrf_safe_public_ip / resolve_ssrf_safe_addresses
    # ------------------------------------------------------------------

    def test_is_ssrf_safe_public_ip_classifies_real_addresses_correctly(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_is_ssrf_safe_public_ip]
        is_safe = sf.is_ssrf_safe_public_ip
        self.assertTrue(is_safe(ipaddress.ip_address("8.8.8.8")))
        self.assertFalse(is_safe(ipaddress.ip_address("127.0.0.1")))  # burn-ignore-ssrf-test-value
        self.assertFalse(is_safe(ipaddress.ip_address("10.0.0.1")))
        self.assertFalse(is_safe(ipaddress.ip_address("169.254.169.254")))
        self.assertFalse(is_safe(ipaddress.ip_address("224.0.0.1")))
        self.assertFalse(is_safe(ipaddress.ip_address("0.0.0.0")))
        self.assertFalse(is_safe(ipaddress.ip_address("::1")))

    def test_resolve_ssrf_safe_addresses_raises_for_a_missing_hostname(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]
        with self.assertRaises(sf.SSRFValidationError):
            sf.resolve_ssrf_safe_addresses("", "test-context")
        with self.assertRaises(sf.SSRFValidationError):
            sf.resolve_ssrf_safe_addresses(None, "test-context")

    def test_resolve_ssrf_safe_addresses_raises_when_unresolvable(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]
        self.safe_patch("socket.getaddrinfo", side_effect=socket.gaierror("nope"))
        with self.assertRaisesRegex(sf.SSRFValidationError, "Could not resolve"):
            sf.resolve_ssrf_safe_addresses("unresolvable.example", "test-context")

    def test_resolve_ssrf_safe_addresses_raises_for_an_unsafe_address(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]
        fake_result = [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("169.254.169.254", 0))]
        self.safe_patch("socket.getaddrinfo", return_value=fake_result)
        with self.assertRaisesRegex(sf.SSRFValidationError, "non-public address"):
            sf.resolve_ssrf_safe_addresses("metadata.example", "test-context")

    def test_resolve_ssrf_safe_addresses_returns_addrinfo_for_a_safe_host(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]
        fake_result = [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("8.8.8.8", 0))]
        mock_getaddrinfo = self.safe_patch("socket.getaddrinfo", return_value=fake_result)
        result = sf.resolve_ssrf_safe_addresses("public.example", "test-context")
        self.assertEqual(result, fake_result)
        mock_getaddrinfo.assert_called_once_with("public.example", None)

    # ------------------------------------------------------------------
    # Layer 2: the pin mechanism itself, isolated from address validation
    # ------------------------------------------------------------------

    def test_pinned_create_connection_ignores_a_different_address_a_second_lookup_would_return(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_pinned_create_connection]
        # Proves the core fix directly: a socket.create_connection-compatible
        # callable built from ONE already-validated address list never calls
        # socket.getaddrinfo() itself (so nothing -- not even a DNS-rebinding
        # attacker's short-TTL second answer -- can substitute a different
        # address for the real connection) and always dials the validated
        # address regardless of what a naive second lookup would have
        # returned.
        marker_server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        marker_server.bind(("127.0.0.1", 0))  # burn-ignore-self-hosted-server: this test spawns its own local marker server and connects to it in the same process
        marker_server.listen(1)
        marker_port = marker_server.getsockname()[1]
        self.addCleanup(marker_server.close)

        accepted = {}

        def _accept_once():
            conn, _addr = marker_server.accept()
            accepted["conn"] = conn

        t = threading.Thread(target=_accept_once, daemon=True)  # burn-ignore-test-daemon-thread: joined with a timeout a few lines below
        t.start()

        validated = [(socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP, "", ("127.0.0.1", marker_port))]  # burn-ignore-self-hosted-server: pins to the local marker server started above
        create_connection = sf._pinned_create_connection(validated)

        def _getaddrinfo_must_not_be_called(*args, **kwargs):
            raise AssertionError(
                "the pinned create_connection callable must never call "
                "socket.getaddrinfo() itself -- doing so would reintroduce "
                "the exact DNS-rebinding TOCTOU this mechanism exists to close"
            )

        self.safe_patch("socket.getaddrinfo", side_effect=_getaddrinfo_must_not_be_called)
        # A hostname/port that resolves nowhere real -- if the pin logic
        # ever fell back to resolving this, the connection would fail
        # (or hit AssertionError above); it must connect to the pinned
        # 127.0.0.1:marker_port instead.
        client_sock = create_connection(("this-hostname-does-not-resolve.invalid", marker_port))
        self.addCleanup(client_sock.close)
        t.join(timeout=5.0)
        self.assertIn("conn", accepted, "the pinned connection never reached the real marker server")
        self.assertEqual(accepted["conn"].getpeername()[1], client_sock.getsockname()[1])
        accepted["conn"].close()

    def test_pinned_create_connection_skips_non_stream_addrinfo_entries(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_pinned_create_connection]
        # resolve_ssrf_safe_addresses() calls getaddrinfo(hostname, None) with
        # no socktype hint (deliberately -- test_resolve_ssrf_safe_addresses_
        # returns_addrinfo_for_a_safe_host above pins that exact call), and on
        # glibc that returns a SOCK_STREAM, a SOCK_DGRAM, AND a SOCK_RAW entry
        # per address, all with the identical IP. Real regression case: a
        # SOCK_STREAM connect() to a CLOSED port fails immediately
        # (connection refused), and if the pin loop fell through to the
        # SOCK_DGRAM entry that follows it in getaddrinfo's own result order,
        # a UDP connect() would "succeed" instantly regardless of whether
        # anything is listening -- silently handing back a UDP socket instead
        # of raising the real connection-refused error. Proves the fix: put a
        # SOCK_DGRAM entry FIRST in the list (worse case than "follows a
        # failed one") pointing at a real, reachable target, and a real
        # SOCK_STREAM entry second pointing at a real listening server; the
        # returned socket must be the real TCP connection, never the UDP one.
        marker_server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        marker_server.bind(("127.0.0.1", 0))  # burn-ignore-self-hosted-server: this test spawns its own local marker server and connects to it in the same process
        marker_server.listen(1)
        marker_port = marker_server.getsockname()[1]
        self.addCleanup(marker_server.close)

        accepted = {}

        def _accept_once():
            conn, _addr = marker_server.accept()
            accepted["conn"] = conn

        t = threading.Thread(target=_accept_once, daemon=True)  # burn-ignore-test-daemon-thread: joined with a timeout a few lines below
        t.start()

        # burn-ignore-self-hosted-server: every address below pins to the local marker server started above, in this same process
        validated = [
            (socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP, "", ("127.0.0.1", marker_port)),  # burn-ignore-self-hosted-server
            (socket.AF_INET, socket.SOCK_RAW, 0, "", ("127.0.0.1", marker_port)),  # burn-ignore-self-hosted-server
            (socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP, "", ("127.0.0.1", marker_port)),  # burn-ignore-self-hosted-server
        ]
        create_connection = sf._pinned_create_connection(validated)
        client_sock = create_connection(("this-hostname-does-not-resolve.invalid", marker_port))
        self.addCleanup(client_sock.close)

        self.assertEqual(client_sock.type, socket.SOCK_STREAM)
        t.join(timeout=5.0)
        self.assertIn("conn", accepted, "the real TCP marker server never accepted a connection")
        self.assertEqual(accepted["conn"].getpeername()[1], client_sock.getsockname()[1])
        accepted["conn"].close()

    # ------------------------------------------------------------------
    # Layer 3: urlopen_ssrf_safe end to end -- real TLS, real Host/SNI
    # ------------------------------------------------------------------

    def test_urlopen_ssrf_safe_pins_the_connection_and_sets_the_real_host_header_and_sni(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]
        # The key regression test: getaddrinfo is mocked to return the real
        # (loopback test-server) address on the FIRST call and a completely
        # different, bogus address on any SUBSEQUENT call -- simulating
        # exactly what a DNS-rebinding attacker's short-TTL second answer
        # would look like. If urlopen_ssrf_safe() ever performed a second,
        # unpinned resolution to actually connect (the pre-fix bug), it
        # would either connect to the bogus address (and this request would
        # fail/hang) or the call-count assertion below would catch it.
        call_log = []
        real_getaddrinfo = socket.getaddrinfo

        def fake_getaddrinfo(host, *args, **kwargs):
            call_log.append(host)
            if host == self.test_hostname:
                return self._validated_addrinfo_for_test_server
            return real_getaddrinfo(host, *args, **kwargs)

        req = urllib.request.Request(f"https://{self.test_hostname}:{self.server_port}/")
        self.safe_patch("socket.getaddrinfo", side_effect=fake_getaddrinfo)
        # is_ssrf_safe_public_ip is stubbed to True here only because a
        # real public IP can't be bound/listened-on in this sandbox -- its
        # own correctness is proven directly by
        # test_is_ssrf_safe_public_ip_classifies_real_addresses_correctly
        # above; this test isolates "does the connection follow the
        # validated address," per the module's own docstring on why these
        # two layers are tested separately.
        self.safe_patch_object(sf, "is_ssrf_safe_public_ip", return_value=True)
        response = sf.urlopen_ssrf_safe(
            req, "test-context", https_only=True, ssl_context=self.client_ssl_context
        )
        with response:
            body = response.read()

        self.assertEqual(body, b"ssrf-safe-fetch-ok")
        self.assertEqual(
            call_log.count(self.test_hostname), 1,
            f"expected exactly ONE getaddrinfo() call for {self.test_hostname}, got {call_log}",
        )
        self.assertEqual(
            _RecordingHandler.recorded_host_headers,
            [f"{self.test_hostname}:{self.server_port}"],
        )
        self.assertEqual(self.__class__.recorded_sni, [self.test_hostname])

    def test_urlopen_ssrf_safe_still_performs_real_tls_hostname_verification(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]
        # Proves the IP-pinning trick does NOT bypass or weaken TLS hostname
        # verification: pinning a MISMATCHED hostname to the exact same
        # server/certificate must still fail verification, since the
        # server's cert is only valid for self.test_hostname's own name.
        wrong_hostname = "wrong-hostname-not-in-cert.invalid"

        def fake_getaddrinfo(host, *args, **kwargs):
            if host == wrong_hostname:
                return self._validated_addrinfo_for_test_server
            raise AssertionError(f"unexpected getaddrinfo() call for {host}")

        req = urllib.request.Request(f"https://{wrong_hostname}:{self.server_port}/")
        self.safe_patch("socket.getaddrinfo", side_effect=fake_getaddrinfo)
        self.safe_patch_object(sf, "is_ssrf_safe_public_ip", return_value=True)
        with self.assertRaises(urllib.error.URLError) as ctx:
            sf.urlopen_ssrf_safe(
                req, "test-context", https_only=True, ssl_context=self.client_ssl_context
            )
        self.assertIsInstance(ctx.exception.reason, ssl.SSLCertVerificationError)

    def test_urlopen_ssrf_safe_rejects_an_unsafe_hostname_before_connecting(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]
        req = urllib.request.Request("https://internal-target.invalid/")
        fake_result = [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("169.254.169.254", 0))]
        self.safe_patch("socket.getaddrinfo", return_value=fake_result)
        with self.assertRaisesRegex(sf.SSRFValidationError, "non-public address"):
            sf.urlopen_ssrf_safe(req, "test-context", https_only=True)

    def test_urlopen_ssrf_safe_https_only_rejects_a_plain_http_redirect_target(self):
        # Tests [@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]
        # https_only=True must not silently allow a plain-http fetch through
        # unpinned -- confirms the plain HTTPHandler is genuinely absent
        # from the opener, not merely unused.
        req = urllib.request.Request("http://plain-http-target.invalid/")
        with self.assertRaises(urllib.error.URLError) as ctx:
            sf.urlopen_ssrf_safe(req, "test-context", https_only=True)
        self.assertIn("unknown url type", str(ctx.exception))
