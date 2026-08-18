# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache


@tagged("post_install", "-at_install")
class TestCustomDomains(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.domain_model = self.env["edge.routing.domain"]
        # cloudflare _inherit's edge.routing.domain and auto-provisions a
        # real Cloudflare custom hostname on create(), requiring a
        # matching website.domain record to exist -- unrelated to what
        # this file tests (domain CRUD/slug resolution), so neutralize it
        # rather than fabricating website fixtures the actual feature
        # under test doesn't need. Guarded so this file still works if
        # cloudflare (not an edge_routing dependency) isn't installed.
        domain_cls = type(self.env["edge.routing.domain"])
        if hasattr(domain_cls, "_create_cloudflare_custom_hostname_batch"):
            self.safe_patch_object(
                domain_cls, "_create_cloudflare_custom_hostname_batch"
            )

    def test_01_domain_crud_and_resolution(self):
        domain = self.domain_model.create(
            {"name": "WWW.TESTCLUB.ORG ", "target_slug": "testclub"}
        )

        self.assertEqual(domain.name, "www.testclub.org")
        self.assertEqual(domain.target_slug, "testclub")

        resolved_slug = self.domain_model.get_target_slug_by_domain("www.testclub.org")
        self.assertEqual(resolved_slug, "testclub")

        domain.write({"target_slug": "newslug"})
        invalidate_model_cache(self.env, "edge.routing.domain")
        resolved_slug = self.domain_model.get_target_slug_by_domain("www.testclub.org")
        self.assertEqual(resolved_slug, "newslug")

        domain.unlink()
        invalidate_model_cache(self.env, "edge.routing.domain")
        resolved_slug = self.domain_model.get_target_slug_by_domain("www.testclub.org")
        self.assertFalse(resolved_slug)
