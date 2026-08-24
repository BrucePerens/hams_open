# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestPagerControllers(HamsHttpCase):
    def test_01_ping_endpoint(self):
        # Tests [@ANCHOR: pd_log_api_i18n]
        response = self.url_open("/api/v1/pager/ping")
        self.assertEqual(
            response.status_code, 200, "Ping endpoint failed to return 200 OK."
        )
        self.assertIn(
            '"status": "ok"',
            response.text,
            "Ping endpoint returned invalid JSON payload.",
        )

    def test_02_board_security_and_render(self):
        # [@ANCHOR: test_pager_board_url]
        # The board should redirect to login for unauthenticated users (auth='user')
        response = self.url_open("/pager/board")
        self.assertTrue(
            "web/login" in response.url,
            'Board endpoint failed to enforce auth="user" security mandate.',
        )

        # Authenticate and check render
        self.env["res.users"].create(
            {
                "name": "Test Ham",
                "login": "tester",
                "password": "testpassword",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.env.flush_all()
        self.authenticate("tester", "testpassword")
        response_auth = self.url_open("/pager/board")
        self.assertEqual(response_auth.status_code, 200)

    def test_03_update_domains_rejects_an_empty_payload(self):
        # update_domains had zero test coverage; this call's own real hmac
        # shared-secret gate had never been exercised end to end.
        response = self.url_open(
            "/api/v1/pager_duty/update_domains",
            json={"jsonrpc": "2.0", "method": "call", "params": {}},
        )
        self.assertEqual(response.json()["result"]["status"], "error")
        self.assertIn("Empty payload", response.json()["result"]["message"])

    def test_04_update_domains_rejects_a_wrong_identity(self):
        # _set_system_param()'s WRITE whitelist doesn't cover this key (only
        # the READ whitelist does, per security_utils.py -- production
        # writes this some other way, e.g. the standard settings UI); a
        # test fixture can set it directly via the raw ORM.
        self.env["ir.config_parameter"].set_param("pager_duty.domain_api_identity", "the-real-secret")
        response = self.url_open(
            "/api/v1/pager_duty/update_domains",
            json={
                "jsonrpc": "2.0",
                "method": "call",
                "params": {"domains": ["example.com"], "api_identity": "wrong-secret"},
            },
        )
        self.assertEqual(response.json()["result"]["status"], "error")
        self.assertIn("Unauthorized", response.json()["result"]["message"])
        self.assertFalse(
            self.env["pager.check"].search([("check_type", "=", "certbot")]),
            "A wrong identity must not be allowed to create/update the certbot check.",
        )

    def test_05_update_domains_fails_closed_when_no_identity_is_configured(self):
        # not hasattr / unset system param -- stored_identity is falsy, so
        # this must reject even a client that happens to send a matching-
        # looking (but meaningless, since nothing real was ever configured)
        # value, not silently accept because "nothing to compare against."
        self.env["ir.config_parameter"].set_param("pager_duty.domain_api_identity", False)
        response = self.url_open(
            "/api/v1/pager_duty/update_domains",
            json={
                "jsonrpc": "2.0",
                "method": "call",
                "params": {"domains": ["example.com"], "api_identity": ""},
            },
        )
        self.assertEqual(response.json()["result"]["status"], "error")
        self.assertIn("Unauthorized", response.json()["result"]["message"])

    def test_06_update_domains_succeeds_with_the_real_identity_and_updates_the_certbot_check(self):
        # _set_system_param()'s WRITE whitelist doesn't cover this key (only
        # the READ whitelist does, per security_utils.py -- production
        # writes this some other way, e.g. the standard settings UI); a
        # test fixture can set it directly via the raw ORM.
        self.env["ir.config_parameter"].set_param("pager_duty.domain_api_identity", "the-real-secret")
        response = self.url_open(
            "/api/v1/pager_duty/update_domains",
            json={
                "jsonrpc": "2.0",
                "method": "call",
                "params": {
                    "domains": ["example.com", "hams.com"],
                    "api_identity": "the-real-secret",
                },
            },
        )
        self.assertEqual(response.json()["result"]["status"], "success")

        check = self.env["pager.check"].search([("check_type", "=", "certbot")], limit=1)
        self.assertTrue(check, "update_domains must create/find the certbot pager.check.")
        self.assertEqual(check.target, "example.com,hams.com")
