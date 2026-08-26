# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import ValidationError
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
        # cloudflare (not an edge_routing dependency) isn't installed --
        # checked via ir.module.module rather than hasattr() probing the
        # class, since hasattr() is banned for masking architectural type
        # uncertainty (matches ham_base/tests/test_config_parameter_security.py's
        # own established pattern for the identical "optional module
        # installed in this combined test run" situation).
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

    def test_02_domain_without_a_dot_is_rejected(self):
        """_check_name's FQDN rule (domain.py) had zero test coverage --
        a bare hostname with no dot must be rejected, not silently
        accepted as a routable custom domain."""
        with self.assertRaises(ValidationError):
            self.domain_model.create({"name": "notadomain", "target_slug": "testclub2"})
            self.env.flush_all()

    def test_03_empty_domain_name_is_rejected(self):
        with self.assertRaises(ValidationError):
            self.domain_model.create({"name": "", "target_slug": "testclub3"})
            self.env.flush_all()

    def test_04_reserved_target_slug_is_rejected_on_create(self):
        """_check_name's reserved-slug rule had zero test coverage -- a
        custom domain must not be mappable onto a reserved route (e.g.
        'shack'), which would let it shadow a real, built-in page."""
        with self.assertRaises(ValidationError):
            self.domain_model.create({"name": "www.hijack.org", "target_slug": "shack"})
            self.env.flush_all()

    def test_05_reserved_target_slug_is_rejected_on_write(self):
        domain = self.domain_model.create(
            {"name": "www.testclub4.org", "target_slug": "testclub4"}
        )
        with self.assertRaises(ValidationError):
            domain.write({"target_slug": "arrl"})
            self.env.flush_all()

    def test_06_reserved_slug_check_is_case_insensitive(self):
        """_check_name lowercases target_slug before comparing against
        RESERVED_SLUGS -- confirm an upper/mixed-case reserved word is
        still caught, not just the exact-case reserved string."""
        with self.assertRaises(ValidationError):
            self.domain_model.create({"name": "www.hijack2.org", "target_slug": "SHACK"})
            self.env.flush_all()
