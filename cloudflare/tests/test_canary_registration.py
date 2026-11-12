# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from cryptography.fernet import Fernet

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

TOKEN = "cfut_Qw8Zk39dQ7mPq2LxV8nTb4RcY6aHj"
SECRET = "0x4AAAAAAAkZq39dQ7mPq2LxV8nTb4RcY6"


@tagged("post_install", "-at_install")
class TestCanaryRegistration(HamsTransactionCase):
    """The Cloudflare token and Turnstile secret are registered with the optional secret canary when it is installed."""

    def setUp(self):
        super().setUp()
        fernet = Fernet(Fernet.generate_key())
        self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet").return_value = fernet
        self.website = self.env["website"].create({"name": "Canary registration site"})

    def _registered(self, value):
        """True when the canary raises an alarm for a response that carries `value`."""
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
            self.env["hams.secret.canary"]._scan_text("<p>%s</p>" % value, "response", "/x")
        self.assertNotIn(value, logs.output[0])
        return True

    # Tests [@ANCHOR: cloudflare:register_secrets_with_canary]
    def test_a_written_token_and_turnstile_secret_are_registered(self):
        if "hams.secret.canary" not in self.env:
            return  # optional addon not installed in this database; the next test covers that case
        self.website.write({"cloudflare_api_token": TOKEN, "cloudflare_turnstile_secret": SECRET})
        self.assertTrue(self._registered(TOKEN))
        self.assertTrue(self._registered(SECRET))

    # Tests [@ANCHOR: cloudflare:register_secrets_with_canary]
    def test_a_short_value_or_the_mask_never_fails_the_save(self):
        self.website.write({"cloudflare_api_token": "short"})
        self.website.write({"cloudflare_turnstile_secret": "********"})
        self.assertEqual(self.website.cloudflare_api_token, "short")
