# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestUserWebsitesGDPRBase(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.user = self.env["res.users"].create(
            {
                "name": "Base GDPR User",
                "login": "base_gdpr",
                "email": "base@example.com",
                "website_slug": "basegdpr",
            }
        )

    def test_01_base_gdpr_dictionary_schema(self):
        """
        Verify the structural integrity of the root GDPR export dictionary.
        Downstream modules (custom_*) rely on this base dictionary existing.
        """
        data = self.user._get_gdpr_export_data()

        self.assertIn("user", data, "The root GDPR export MUST contain a 'user' key.")
        self.assertIn("name", data["user"])
        self.assertIn("email", data["user"])
        self.assertIn("website_slug", data["user"])

        self.assertEqual(data["user"]["name"], "Base GDPR User")
        self.assertEqual(data["user"]["email"], "base@example.com")
        self.assertEqual(data["user"]["website_slug"], "basegdpr")
