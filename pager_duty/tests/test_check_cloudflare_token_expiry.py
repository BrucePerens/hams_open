# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import datetime
import json
import os
import tempfile
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import check_cloudflare_token_expiry as check_mod


def _write_credentials(path, token="fake-token-value"):
    with open(path, "w", encoding="utf-8") as f:
        f.write(f"dns_cloudflare_api_token = {token}\n")


def _cf_verify_response(expires_on=None, success=True):
    result = {}
    if expires_on:
        result["expires_on"] = expires_on
    body = {"success": success, "result": result, "errors": [] if success else [{"message": "bad token"}]}
    return json.dumps(body).encode("utf-8")


@tagged("post_install", "-at_install")
class TestCloudflareTokenExpiryCheck(HamsTransactionCase):
    """
    LOCAL_RELAY_TLS_CERTIFICATE.md item 4's second half: a `pager.check`
    "synthetic" script verifying the Cloudflare API token
    `relay_cert_renew` depends on isn't about to expire. No live network
    call and no real credential -- urlopen is mocked, and the credentials
    file is a throwaway temp file, not the real
    /opt/hams/etc/relay_cert_renew/cloudflare.ini.
    """

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.cred_path = os.path.join(self.tmpdir.name, "cloudflare.ini")
        self.orig_env = dict(os.environ)
        self.addCleanup(lambda: os.environ.clear() or os.environ.update(self.orig_env))

    def test_01_healthy_token_far_from_expiry_exits_zero(self):
        _write_credentials(self.cred_path)
        os.environ["HAMS_CLOUDFLARE_CREDENTIALS_PATH"] = self.cred_path

        future = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=300)).isoformat()
        mock_resp = MagicMock()
        mock_resp.read.return_value = _cf_verify_response(expires_on=future)
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_cloudflare_token_expiry.urllib.request.urlopen"
        )
        mock_urlopen.return_value.__enter__.return_value = mock_resp

        self.assertEqual(check_mod.main(), 0)

    def test_02_token_close_to_expiry_exits_nonzero(self):
        _write_credentials(self.cred_path)
        os.environ["HAMS_CLOUDFLARE_CREDENTIALS_PATH"] = self.cred_path
        os.environ["HAMS_CLOUDFLARE_TOKEN_WARN_DAYS"] = "30"

        soon = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=5)).isoformat()
        mock_resp = MagicMock()
        mock_resp.read.return_value = _cf_verify_response(expires_on=soon)
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_cloudflare_token_expiry.urllib.request.urlopen"
        )
        mock_urlopen.return_value.__enter__.return_value = mock_resp

        self.assertEqual(check_mod.main(), 1)

    def test_03_no_expiry_set_is_healthy_not_a_failure(self):
        """A real Cloudflare token can have no expiry at all -- must not be treated as an error."""
        _write_credentials(self.cred_path)
        os.environ["HAMS_CLOUDFLARE_CREDENTIALS_PATH"] = self.cred_path

        mock_resp = MagicMock()
        mock_resp.read.return_value = _cf_verify_response(expires_on=None)
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_cloudflare_token_expiry.urllib.request.urlopen"
        )
        mock_urlopen.return_value.__enter__.return_value = mock_resp

        self.assertEqual(check_mod.main(), 0)

    def test_04_invalid_token_exits_nonzero(self):
        _write_credentials(self.cred_path, token="revoked-token")
        os.environ["HAMS_CLOUDFLARE_CREDENTIALS_PATH"] = self.cred_path

        mock_resp = MagicMock()
        mock_resp.read.return_value = _cf_verify_response(success=False)
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_cloudflare_token_expiry.urllib.request.urlopen"
        )
        mock_urlopen.return_value.__enter__.return_value = mock_resp

        self.assertEqual(check_mod.main(), 1)

    def test_05_missing_credentials_file_exits_nonzero_not_raises(self):
        """The real permission boundary this script depends on: if the file genuinely can't be
        read (missing, or a real permission denial in production), fail cleanly -- never a raw
        traceback out of a monitoring check."""
        os.environ["HAMS_CLOUDFLARE_CREDENTIALS_PATH"] = os.path.join(
            self.tmpdir.name, "does_not_exist.ini"
        )
        self.assertEqual(check_mod.main(), 1)

    def test_06_malformed_credentials_file_exits_nonzero_not_raises(self):
        with open(self.cred_path, "w", encoding="utf-8") as f:
            f.write("this is not a valid credentials file at all\n")
        os.environ["HAMS_CLOUDFLARE_CREDENTIALS_PATH"] = self.cred_path
        self.assertEqual(check_mod.main(), 1)
