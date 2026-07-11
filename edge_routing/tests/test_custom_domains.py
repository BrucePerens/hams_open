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
