# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import contextlib
import datetime
import io
import os
import subprocess
import tempfile

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import check_auth_cert_expiry as check_mod


def _make_cert(directory, days, name="auth.crt"):
    """A throwaway self-signed certificate valid for `days` days, made with the real openssl. The key is
    generated beside it in the temp directory and is not used for anything but signing this fixture."""
    cert = os.path.join(directory, name)
    key = os.path.join(directory, name + ".key")
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes",
         "-keyout", key, "-out", cert, "-days", str(days), "-subj", "/CN=auth.hams.com"],
        check=True, capture_output=True,
    )
    return cert


@tagged("post_install", "-at_install")
class TestAuthCertSeverity(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:auth_cert_severity]
    def test_healthy_at_and_past_thirty_days(self):
        self.assertIsNone(check_mod.severity_for_auth_cert(30))
        self.assertIsNone(check_mod.severity_for_auth_cert(90))

    def test_warning_under_thirty_days_does_not_page(self):
        self.assertEqual(check_mod.severity_for_auth_cert(29.9), "low")
        self.assertEqual(check_mod.severity_for_auth_cert(14), "low")

    def test_pages_under_fourteen_days(self):
        # `high` is the lowest severity that pages on-call (incident.py: low/medium are trend-tracked only),
        # so the shared 14-days-is-medium ladder would not have met the requirement.
        self.assertEqual(check_mod.severity_for_auth_cert(13.9), "high")
        self.assertEqual(check_mod.severity_for_auth_cert(2), "high")
        self.assertNotIn(check_mod.severity_for_auth_cert(13.9), ("low", "medium"))

    def test_critical_under_two_days_and_when_expired(self):
        self.assertEqual(check_mod.severity_for_auth_cert(1.5), "critical")
        self.assertEqual(check_mod.severity_for_auth_cert(-3), "critical")


@tagged("post_install", "-at_install")
class TestAuthCertExpiryCheck(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.orig_env = dict(os.environ)
        self.addCleanup(lambda: os.environ.clear() or os.environ.update(self.orig_env))

    def _run_main(self):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = check_mod.main()
        return code, out.getvalue(), err.getvalue()

    # Tests [@ANCHOR: pager_duty:auth_cert_parse_enddate]
    def test_parse_enddate_handles_openssl_padding(self):
        parsed = check_mod.parse_enddate("notAfter=Jan  5 03:04:05 2027 GMT\n")
        self.assertEqual(parsed, datetime.datetime(2027, 1, 5, 3, 4, 5, tzinfo=datetime.timezone.utc))
        parsed = check_mod.parse_enddate("notAfter=Dec 20 23:59:59 2026 GMT\n")
        self.assertEqual(parsed.day, 20)
        with self.assertRaises(ValueError):
            check_mod.parse_enddate("garbage")

    # Tests [@ANCHOR: pager_duty:auth_cert_main]
    # Tests [@ANCHOR: pager_duty:auth_cert_read_expiry]
    def test_healthy_certificate_exits_zero(self):
        os.environ["HAMS_AUTH_CERT_PATH"] = _make_cert(self.tmpdir.name, 90)
        code, out, err = self._run_main()
        self.assertEqual(code, 0, err)
        self.assertIn("healthy", out)
        self.assertEqual(err, "")

    def test_under_thirty_days_warns_with_low_severity(self):
        os.environ["HAMS_AUTH_CERT_PATH"] = _make_cert(self.tmpdir.name, 20)
        code, _, err = self._run_main()
        self.assertEqual(code, 1)
        self.assertIn("SEVERITY:low", err)

    def test_under_fourteen_days_pages_with_high_severity(self):
        os.environ["HAMS_AUTH_CERT_PATH"] = _make_cert(self.tmpdir.name, 10)
        code, _, err = self._run_main()
        self.assertEqual(code, 1)
        self.assertIn("SEVERITY:high", err)
        self.assertIn("expires in", err)

    def test_an_unreadable_certificate_pages(self):
        path = os.path.join(self.tmpdir.name, "bad.crt")
        with open(path, "w", encoding="utf-8") as f:
            f.write("not a certificate\n")
        os.environ["HAMS_AUTH_CERT_PATH"] = path
        code, _, err = self._run_main()
        self.assertEqual(code, 1)
        self.assertIn("SEVERITY:high", err)

    def test_missing_file_falls_back_to_the_served_certificate(self):
        pem = open(_make_cert(self.tmpdir.name, 10), encoding="utf-8").read()
        os.environ["HAMS_AUTH_CERT_PATH"] = os.path.join(self.tmpdir.name, "absent.crt")
        mock_get = self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_auth_cert_expiry.ssl.get_server_certificate", return_value=pem
        )
        code, _, err = self._run_main()
        mock_get.assert_called_once()
        self.assertEqual(mock_get.call_args[0][0], ("auth.hams.com", 443))
        self.assertEqual(code, 1)
        self.assertIn("SEVERITY:high", err)
        self.assertIn("served by auth.hams.com:443", err)

    def test_file_and_network_both_unavailable_pages(self):
        os.environ["HAMS_AUTH_CERT_PATH"] = os.path.join(self.tmpdir.name, "absent.crt")
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_auth_cert_expiry.ssl.get_server_certificate",
            side_effect=OSError("connection refused"),
        )
        code, _, err = self._run_main()
        self.assertEqual(code, 1)
        self.assertIn("SEVERITY:high", err)
