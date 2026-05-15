# -*- coding: utf-8 -*-
from unittest.mock import patch
from odoo.tests.common import TransactionCase, tagged
from odoo.exceptions import UserError


@tagged("post_install", "-at_install")
class TestWafManagement(TransactionCase):

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        self.website = self.env["website"].get_current_website()
        self.website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.ban_ip")
    def test_01_cf_execute_ban(self, mock_ban_ip):
        # [@ANCHOR: test_cf_execute_ban]
        # Tests [@ANCHOR: cf_execute_ban]
        mock_ban_ip.return_value = (True, "fake_rule_123")

        res = (
            self.env["cloudflare.ip.ban"]
            .with_user(self.svc_uid)
            ._execute_ban("10.0.0.1", notes="Test Spam", website_id=self.website.id)
        )
        self.assertTrue(res)

        ban_record = self.env["cloudflare.ip.ban"].search(
            [("ip_address", "=", "10.0.0.1")], limit=1
        )
        self.assertTrue(ban_record)
        self.assertEqual(ban_record.state, "active")
        self.assertEqual(ban_record.cf_rule_id, "fake_rule_123")
        self.assertEqual(ban_record.website_id.id, self.website.id)

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.unban_ip")
    def test_02_cf_action_lift_ban(self, mock_unban_ip):
        # [@ANCHOR: test_cf_action_lift_ban]
        # Tests [@ANCHOR: cf_action_lift_ban]
        ban_record = self.env["cloudflare.ip.ban"].create(
            {
                "ip_address": "192.168.1.50",
                "cf_rule_id": "rule_999",
                "state": "active",
                "website_id": self.website.id,
            }
        )

        mock_unban_ip.return_value = (False, "Edge Offline")
        with self.assertRaises(UserError):
            ban_record.action_lift_ban()
        self.assertEqual(ban_record.state, "active")

        mock_unban_ip.return_value = (True, "Success")
        ban_record.action_lift_ban()
        self.assertEqual(ban_record.state, "lifted")

    @patch("odoo.addons.cloudflare.models.config_manager.get_zone_ruleset")
    def test_03_cf_action_pull_waf_rules(self, mock_get_ruleset):
        self.env["cloudflare.waf.rule"].create(
            {
                "name": "Old Rule",
                "expression": 'http.request.uri == "/"',
                "website_id": self.website.id,
            }
        )

        mock_get_ruleset.return_value = {
            "rules": [
                {
                    "id": "abc",
                    "description": "Cloudflare Rule 1",
                    "action": "block",
                    "expression": "ip.src eq 1.1.1.1",
                    "enabled": True,
                }
            ]
        }

        success, msg = (
            self.env["cloudflare.config.manager"]
            .with_user(self.svc_uid)
            .action_pull_waf_rules(website_id=self.website.id)
        )
        self.assertTrue(success)

        rules = self.env["cloudflare.waf.rule"].search(
            [("website_id", "=", self.website.id)]
        )
        self.assertEqual(len(rules), 1)
        self.assertEqual(rules[0].name, "Cloudflare Rule 1")

    @patch("odoo.addons.cloudflare.models.config_manager.create_zone_ruleset")
    @patch("odoo.addons.cloudflare.models.config_manager.update_zone_ruleset")
    @patch("odoo.addons.cloudflare.models.config_manager.get_zone_ruleset")
    def test_04_cf_action_push_waf_rules(self, mock_get, mock_update, mock_create):
        self.env["cloudflare.waf.rule"].search([]).unlink()
        self.env["cloudflare.waf.rule"].create(
            {
                "name": "Local Rule",
                "action": "managed_challenge",
                "expression": "ip.src eq 2.2.2.2",
                "website_id": self.website.id,
            }
        )

        mock_get.return_value = {"id": "ruleset_777"}
        mock_update.return_value = (True, "Updated")

        success, msg = (
            self.env["cloudflare.config.manager"]
            .with_user(self.svc_uid)
            .action_push_waf_rules(website_id=self.website.id)
        )
        self.assertTrue(success)
        mock_update.assert_called_once()

        payload = mock_update.call_args[0][1]
        self.assertEqual(payload["rules"][0]["action"], "managed_challenge")
