# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from unittest.mock import patch

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDomainsApi(HamsHttpCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # cloudflare _inherit's edge.routing.domain and auto-provisions a
        # real Cloudflare custom hostname on create(), requiring a
        # matching website.domain record to exist -- unrelated to what
        # this file tests (the domains API endpoint), so neutralize it
        # rather than fabricating a website fixture the feature under
        # test doesn't need. Guarded so this file still works if
        # cloudflare (not an edge_routing dependency) isn't installed.
        domain_cls = type(cls.env["edge.routing.domain"])
        if hasattr(domain_cls, "_create_cloudflare_custom_hostname_batch"):
            patcher = patch.object(
                domain_cls, "_create_cloudflare_custom_hostname_batch"
            )
            patcher.start()
            cls.addClassCleanup(patcher.stop)

        # Ensure there is a domain in edge.routing.domain. Only
        # base.group_system has create access to edge.routing.domain (even
        # edge_routing's own service account group is read-only), so this
        # must be created as a real admin, not a service account.
        cls.env["edge.routing.domain"].with_user(cls.env.ref("base.user_admin")).create(
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
