# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
"""
Real coverage for EdgeRoutingMixin.get_record_by_domain() -- had zero test coverage before this
file (confirmed by grepping tests/ for the method name). Its two halves are each already tested
separately: edge.routing.domain.get_target_slug_by_domain() by test_custom_domains.py,
get_record_by_slug() by test_routing_mixin.py -- but nothing ever verified the composition
(domain -> slug -> record) that get_record_by_domain() itself exists to provide, which is the
actual entry point real custom-domain website routing would call.
"""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestGetRecordByDomain(HamsTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.env = cls.env(context=dict(cls.env.context, tracking_disable=True))
        cls.User = cls.env["res.users"]
        cls.Domain = cls.env["edge.routing.domain"]

    def test_a_domain_resolves_through_its_slug_to_the_matching_records_id(self):
        user = self.User.create(
            {"name": "Domain Routed User", "login": "domain_routed@example.com"}
        )
        self.Domain.create({"name": "www.example-club.org", "target_slug": user.website_slug})

        record_id = self.User.get_record_by_domain("www.example-club.org")
        self.assertEqual(record_id, user.id)

    def test_an_unknown_domain_returns_false(self):
        self.assertFalse(self.User.get_record_by_domain("www.never-registered.example"))

    def test_a_domain_pointing_at_a_slug_nothing_owns_returns_false(self):
        # The domain itself resolves fine (get_target_slug_by_domain succeeds), but no
        # res.users record has that website_slug -- the second half of the composition
        # must also fail closed, not raise or return a stale/wrong id.
        self.Domain.create(
            {"name": "www.orphaned-slug.org", "target_slug": "nobody-has-this-slug"}
        )
        self.assertFalse(self.User.get_record_by_domain("www.orphaned-slug.org"))

    def test_a_falsy_domain_short_circuits_without_a_query(self):
        self.assertFalse(self.User.get_record_by_domain(""))
        self.assertFalse(self.User.get_record_by_domain(False))
