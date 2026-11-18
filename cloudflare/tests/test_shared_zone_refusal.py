# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""A website that lists cloudflare_purge_hosts shares its Cloudflare zone (hams.com served as a custom hostname of the
perens.com zone): no zone-wide push may be made for it, and nothing is read or written at Cloudflare to find that out."""
from cryptography.fernet import Fernet

from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

CM = "odoo.addons.cloudflare.models.config_manager."
RL = "odoo.addons.cloudflare.models.rate_limit_push."
ZS = "odoo.addons.cloudflare.models.zone_settings_wizard."


@tagged("post_install", "-at_install")
class TestSharedZoneRefusal(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet").return_value = Fernet(
            Fernet.generate_key())
        self.website = self.env["website"].create({"name": "Shared Zone Site", "domain": "https://shared.example"})
        self.website.write({"cloudflare_api_token": "tok", "cloudflare_zone_id": "z"})
        self.manager = self.env["cloudflare.config.manager"]

    def _share(self):
        self.website.write({"cloudflare_purge_hosts": "shared.example, www.shared.example"})

    def test_a_website_with_its_own_zone_is_not_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
        self.assertEqual(self.website._cloudflare_shared_zone_refusal(), "")

    def test_every_zone_wide_push_is_refused_without_a_call_to_cloudflare(self):
        # Tests [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
        # [@ANCHOR: cloudflare:COMM_test_shared_zone_refusal]
        self._share()
        self.assertIn("shares its Cloudflare zone", self.website._cloudflare_shared_zone_refusal())
        reads = [self.safe_patch(CM + name) for name in ("get_zone_ruleset_checked", "update_zone_ruleset",
                                                          "create_zone_ruleset")]
        reads.append(self.safe_patch(RL + "get_zone_ruleset_checked"))
        reads.append(self.safe_patch(CM + "update_zone_setting"))
        ok, message = self.manager.action_push_waf_rules(website_id=self.website.id)
        self.assertFalse(ok)
        self.assertIn("shares its Cloudflare zone", message)
        for plan in (self.manager.plan_cache_rules(website_id=self.website.id),
                     self.manager.plan_zone_settings(website_id=self.website.id),
                     self.manager.plan_rate_limits(website_id=self.website.id)):
            self.assertFalse(plan["ok"])
            self.assertIn("shares its Cloudflare zone", plan["message"])
        self.assertFalse(self.manager.action_push_cache_rules(website_id=self.website.id)[0])
        self.assertFalse(self.manager.action_push_zone_settings(website_id=self.website.id)[0])
        self.assertFalse(self.manager.action_push_rate_limits(website_id=self.website.id)[0])
        for mock in reads:
            mock.assert_not_called()

    def test_the_zone_settings_wizard_refuses_to_apply(self):
        # Tests [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
        self._share()
        update = self.safe_patch(ZS + "update_zone_setting")
        wizard = self.env["cloudflare.zone.settings.wizard"].create({"website_id": self.website.id, "security_level": "high"})
        with self.assertRaises(UserError):
            wizard.action_apply_settings()
        update.assert_not_called()

    def test_initialising_the_edge_state_skips_a_shared_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_initialize_cloudflare_state]
        self._share()
        read = self.safe_patch(CM + "get_zone_ruleset_checked", return_value=("error", "down"))
        self.manager.initialize_cloudflare_state()
        for call in read.call_args_list:
            self.assertNotEqual(call.args[2], "z", "the shared zone was read")

    def test_pull_waf_rules_is_refused_for_a_shared_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
        self._share()
        read = self.safe_patch(CM + "get_zone_ruleset_checked")
        ok, message = self.manager.action_pull_waf_rules(website_id=self.website.id)
        self.assertFalse(ok)
        self.assertIn("shares its Cloudflare zone", message)
        read.assert_not_called()

    def test_ip_ban_is_refused_for_a_shared_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
        self._share()
        ban = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.ban_ip", return_value=(True, "rule"))
        self.assertFalse(self.env["cloudflare.ip.ban"]._execute_ban("192.0.2.9", website_id=self.website.id))
        ban.assert_not_called()
        row = self.env["cloudflare.ip.ban"].search([("ip_address", "=", "192.0.2.9")])
        self.assertEqual(row.state, "failed")
        self.assertIn("shares its Cloudflare zone", row.notes)

    def test_ip_ban_still_works_for_an_own_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
        ban = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.ban_ip", return_value=(True, "rule"))
        self.assertTrue(self.env["cloudflare.ip.ban"]._execute_ban("192.0.2.10", website_id=self.website.id))
        ban.assert_called_once()
