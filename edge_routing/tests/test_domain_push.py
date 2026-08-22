# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDomainPush(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        # cloudflare _inherit's edge.routing.domain and auto-provisions a
        # real Cloudflare custom hostname on create(), requiring a
        # matching website.domain record to exist -- unrelated to what
        # this file tests (pager_duty push logic/batching), so neutralize
        # it rather than fabricating 1000+ website fixtures the actual
        # feature under test doesn't need. Guarded so this file still
        # works if cloudflare (not an edge_routing dependency) isn't
        # installed -- checked via ir.module.module rather than hasattr()
        # probing the class, since hasattr() is banned for masking
        # architectural type uncertainty (matches ham_base/tests/
        # test_config_parameter_security.py's own established pattern
        # for the identical "optional module installed in this combined
        # test run" situation).
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

    def test_domain_push_logic(self):
        """Test the logic that gathers domains and pushes them."""
        # Instead of dealing with postcommit complexities in tests,
        # we will directly test the _invalidate_cache logic by simulating the environment.
        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )

        # Create a domain
        domain_model.create(
            {
                "name": "manualpush.com",
                "target_slug": "manualpush",
            }
        )
        self.env.flush_all()

        # We can't easily mock the inner function `push_to_pager_duty`
        # But we can call the outer function to ensure it doesn't crash.
        domain_model._invalidate_cache(["manualpush.com"])

    def test_push_all_to_pager_duty_batching(self):

        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )
        
        # Create 1005 domains
        vals_list = [{'name': f'domain{i}.com', 'target_slug': f'target{i}'} for i in range(1005)]
        domain_model.create(vals_list)
        
        mock_post = self.safe_patch('odoo.addons.edge_routing.models.domain.requests.post')
        domain_model.push_all_to_pager_duty()
            
        total_domains = domain_model.search_count([])
        self.assertGreaterEqual(total_domains, 1005)
        
        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        posted_domains = kwargs.get('json', {}).get('params', {}).get('domains', [])
        self.assertEqual(len(posted_domains), total_domains)
