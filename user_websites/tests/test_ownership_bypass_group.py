# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
"""Regression guards for user_websites.group_user_websites_ownership_bypass.

Service-account privilege audit, 2026-09-14 (bug-hunt): UserWebsitesOwnedMixin's two ownership
checks used to treat membership in user_websites.group_user_websites_service_account -- the full
provisioning group, carrying create/write/unlink on res.groups, ir.ui.view, website.rewrite,
mail.followers, discuss.channel.member and more -- as their "may bypass the ownership rules"
flag. Four or five functionally unrelated service accounts in other modules had joined that group
purely to get the one boolean, silently inheriting the whole ACL. The bypass is now its own
capability-only group. These tests pin down the three properties that make the split real:

1. the bypass group has NO grants of its own, anywhere (no ir.model.access, no ir.rule, no
   implied groups) -- the moment someone adds one, it stops being a pure capability and this
   test is the mechanical alarm;
2. the provisioning group still implies it, so the provisioning account itself keeps passing
   the mixin's checks with no second explicit membership;
3. the mixin genuinely keys on the bypass group: an actor holding only that group passes both
   checks, an otherwise-identical actor without it is refused -- so the test discriminates the
   real behavior, not just the group's existence.
"""
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import AccessError

BYPASS = "user_websites.group_user_websites_ownership_bypass"
PROVISIONING = "user_websites.group_user_websites_service_account"


@tagged("post_install", "-at_install", "security")
class TestOwnershipBypassGroup(RealTransactionCase):

    def setUp(self):
        super().setUp()
        portal = self.env.ref("base.group_portal").id
        self.other_user = self.env["res.users"].create(
            {
                "name": "Bypass Audit Other User",
                "login": "bypass_audit_other",
                "email": "bypass_audit_other@example.com",
                "group_ids": [(6, 0, [portal])],
            }
        )
        # Two otherwise-identical portal users; only one holds the bypass group.
        self.holder = self.env["res.users"].create(
            {
                "name": "Bypass Audit Holder",
                "login": "bypass_audit_holder",
                "email": "bypass_audit_holder@example.com",
                "group_ids": [(6, 0, [portal, self.env.ref(BYPASS).id])],
            }
        )
        self.non_holder = self.env["res.users"].create(
            {
                "name": "Bypass Audit Non-Holder",
                "login": "bypass_audit_non_holder",
                "email": "bypass_audit_non_holder@example.com",
                "group_ids": [(6, 0, [portal])],
            }
        )

    # [@ANCHOR: test_ownership_bypass_group_is_capability_only]
    def test_bypass_group_carries_no_grants_of_its_own(self):
        group = self.env.ref(BYPASS)
        acl = self.env["ir.model.access"].search([("group_id", "=", group.id)])
        self.assertFalse(acl, "bypass group must carry no ACL rows: %s" % acl.mapped("name"))
        rules = self.env["ir.rule"].search([("groups", "in", group.ids)])
        self.assertFalse(rules, "bypass group must carry no ir.rule: %s" % rules.mapped("name"))
        self.assertFalse(group.implied_ids, group.implied_ids.mapped("name"))

    # [@ANCHOR: test_ownership_bypass_group_implied_by_provisioning]
    def test_provisioning_group_and_account_still_carry_the_bypass(self):
        self.assertIn(self.env.ref(BYPASS), self.env.ref(PROVISIONING).all_implied_ids)
        provisioner = self.env.ref("user_websites.user_websites_service_account")
        self.assertTrue(provisioner.has_group(BYPASS))

    # [@ANCHOR: test_ownership_bypass_group_gates_mixin_write]
    # Tests [@ANCHOR: mixin_proxy_ownership_write]
    def test_write_check_keys_on_bypass_group(self):
        Page = self.env["website.page"]  # inherits user_websites.owned.mixin
        vals = {"owner_user_id": self.other_user.id}
        # Empty recordset on purpose: the privileged branch of the write check depends only
        # on env.user's groups, so this isolates exactly the group test from any ACL noise.
        Page.with_user(self.holder)._check_proxy_ownership_write(vals)
        with self.assertRaises(AccessError):
            Page.with_user(self.non_holder)._check_proxy_ownership_write(vals)

    # [@ANCHOR: test_ownership_bypass_group_gates_mixin_create]
    # Tests [@ANCHOR: mixin_proxy_ownership_create]
    def test_create_check_keys_on_bypass_group(self):
        Page = self.env["website.page"]
        vals_list = [{"owner_user_id": self.other_user.id}]
        Page.with_user(self.holder)._check_proxy_ownership_create(vals_list)
        with self.assertRaises(AccessError):
            Page.with_user(self.non_holder)._check_proxy_ownership_create(
                [{"owner_user_id": self.other_user.id}]
            )
