# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDomainsApi(HamsHttpCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # Ensure there is a domain in edge.routing.domain
        svc_uid = cls.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        cls.env["edge.routing.domain"].with_user(svc_uid).create(
            {
                "name": "testdomain1.com",
                "target_slug": "test-slug-1",
            }
        )
        cls.env.flush_all()

    def test_domains_api_returns_all_domains(self):
        """Test that the /api/v1/user_websites/domains endpoint returns all domains."""
        response = self.url_open("/api/v1/user_websites/domains")
        self.assertEqual(response.status_code, 200)

        data = response.json()
        self.assertIn("domains", data)
        domains = data["domains"]

        self.assertIn("testdomain1.com", domains)
