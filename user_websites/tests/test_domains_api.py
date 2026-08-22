# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDomainsApi(HamsHttpCase):

    def setUp(self):
        super().setUp()
        # cloudflare _inherit's edge.routing.domain and auto-provisions a
        # real Cloudflare custom hostname on create(), requiring a
        # matching website.domain record to exist -- unrelated to what
        # this file tests (the domains API endpoint), so neutralize it
        # rather than fabricating a website fixture the feature under
        # test doesn't need. Guarded so this file still works if
        # cloudflare (not an edge_routing dependency) isn't installed --
        # checked via ir.module.module rather than hasattr() probing the
        # class, since hasattr() is banned for masking architectural type
        # uncertainty (matches ham_base/tests/test_config_parameter_security.py's
        # own established pattern for the identical "optional module
        # installed in this combined test run" situation). Moved from
        # setUpClass to setUp (this file has only one test method, so
        # there's no per-class-fixture cost) since safe_patch_object() is
        # an instance method and setUpClass has no self.
        acl_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.odoo_facility_service_internal"
        )
        cloudflare_installed = bool(
            self.env["ir.module.module"]
            .with_user(acl_svc_uid)
            .search([("name", "=", "cloudflare"), ("state", "=", "installed")], limit=1)
        )
        if cloudflare_installed:
            domain_cls = type(self.env["edge.routing.domain"])
            self.safe_patch_object(
                domain_cls, "_create_cloudflare_custom_hostname_batch"
            )

        # Ensure there is a domain in edge.routing.domain. Only
        # base.group_system has create access to edge.routing.domain (even
        # edge_routing's own service account group is read-only), so this
        # must be created as a real admin, not a service account.
        self.env["edge.routing.domain"].with_user(self.env.ref("base.user_admin")).create(
            {
                "name": "testdomain1.com",
                "target_slug": "test-slug-1",
            }
        )
        self.env.flush_all()

    def test_domains_api_returns_all_domains(self):
        # Tests [@ANCHOR: COMM_test_domains_api_returns_all_domains]
        """Test that the /api/v1/user_websites/domains endpoint returns all domains."""
        response = self.url_open("/api/v1/user_websites/domains")
        self.assertEqual(response.status_code, 200)

        data = response.json()
        self.assertIn("domains", data)
        domains = data["domains"]

        self.assertIn("testdomain1.com", domains)
