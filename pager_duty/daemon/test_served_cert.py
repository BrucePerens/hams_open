# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The served-certificate expiry check of the "certbot" check type. Plain unittest, no Odoo (needs the openssl tool):
    python3 pager_duty/daemon/test_served_cert.py
"""
# Tests [@ANCHOR: pager_duty:served_cert_problem]
import os
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


def serve_once(days, tmp):
    """Serve one TLS connection on a free local port with a self-signed certificate valid for `days` days."""
    crt, key = os.path.join(tmp, f"{days}.crt"), os.path.join(tmp, f"{days}.key")
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", str(days), "-subj", "/CN=a.test",
         "-keyout", key, "-out", crt],
        check=True, capture_output=True,
    )
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(crt, key)
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))  # burn-ignore-self-hosted-server: the test spawns its own local TLS server
    srv.listen(1)

    def run():
        conn, _ = srv.accept()
        try:
            ctx.wrap_socket(conn, server_side=True).close()
        except OSError as e:  # the client may hang up early; the test asserts on the client side
            print(f"test server: {e}", file=sys.stderr)
        srv.close()

    threading.Thread(target=run, daemon=True).start()
    return srv.getsockname()[1]


class WildcardProbeTests(unittest.TestCase):
    # Tests [@ANCHOR: pager_duty:certbot_probe_name]
    def test_wildcard_is_probed_under_a_fixed_label(self):
        self.assertEqual(gm.certbot_probe_name("*.hams.com"), "hams-cert-check.hams.com")
        self.assertEqual(gm.certbot_probe_name("w7com.hams.com"), "w7com.hams.com")

    def test_wildcard_target_checks_the_probe_name(self):
        seen = []
        with mock.patch.object(gm.urllib.request, "urlopen") as urlopen, \
                mock.patch.object(gm, "certbot_domain_problem", side_effect=lambda d, ip: seen.append(d)), \
                mock.patch.object(gm, "served_cert_problem", side_effect=lambda d: seen.append(d) or "soon") as served:
            urlopen.return_value.__enter__.return_value.status = 200
            urlopen.return_value.__enter__.return_value.read.return_value = b"198.51.100.1"
            ok, msg = gm.execute_check({"type": "certbot", "target": "*.hams.com"})
        self.assertFalse(ok)
        self.assertEqual(seen, ["hams-cert-check.hams.com", "hams-cert-check.hams.com"])
        served.assert_called_once_with("hams-cert-check.hams.com")


class ServedCertTests(unittest.TestCase):
    def test_long_lived_certificate_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIsNone(gm.served_cert_problem("a.test", port=serve_once(60, tmp)))

    def test_near_expiry_certificate_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            msg = gm.served_cert_problem("a.test", port=serve_once(5, tmp))
        self.assertIn("a.test", msg)
        self.assertIn("under 14 days", msg)

    def test_nothing_listening_fails(self):
        s = socket.socket()
        s.bind(("127.0.0.1", 0))  # burn-ignore-self-hosted-server: a free local port, nothing listens on it
        port = s.getsockname()[1]
        s.close()
        self.assertIn("Could not read", gm.served_cert_problem("a.test", port=port))


if __name__ == "__main__":
    unittest.main()
