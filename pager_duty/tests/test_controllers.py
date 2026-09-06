# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestPagerControllers(HamsHttpCase):
    def test_01_ping_endpoint(self):
        # Tests [@ANCHOR: pd_log_api_i18n]

        # Tests [@ANCHOR: pager_duty:ping]
        response = self.url_open("/api/v1/pager/ping")
        self.assertEqual(
            response.status_code, 200, "Ping endpoint failed to return 200 OK."
        )
        self.assertIn(
            '"status": "ok"',
            response.text,
            "Ping endpoint returned invalid JSON payload.",
        )

    def test_01b_heartbeat_endpoint_updates_last_heartbeat_and_check_heartbeat_rpc_sees_it(self):
        # Tests [@ANCHOR: pager_duty:heartbeat]

        # Tests [@ANCHOR: pager_duty:check_heartbeat_rpc]

        # Tests [@ANCHOR: pager_duty:get_check_id_by_uuid]
        check = self.env["pager.check"].create(
            {"name": "Heartbeat Push Monitor", "check_type": "heartbeat", "interval": 60}
        )
        response = self.url_open(f"/api/v1/pager/heartbeat/{check.heartbeat_uuid}")
        self.assertEqual(response.status_code, 200)
        self.assertIn('"status": "ok"', response.text)

        check.invalidate_recordset(["last_heartbeat"])
        self.assertTrue(check.last_heartbeat)
        self.assertTrue(
            self.env["pager.check"].check_heartbeat_rpc(check.heartbeat_uuid, 60),
            "A fresh heartbeat within the interval must report healthy.",
        )

    def test_01c_heartbeat_endpoint_404s_on_an_unknown_uuid(self):
        # Tests [@ANCHOR: pager_duty:heartbeat]

        # Tests [@ANCHOR: pager_duty:get_check_id_by_uuid]
        response = self.url_open("/api/v1/pager/heartbeat/no-such-uuid")
        self.assertEqual(response.status_code, 404)

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

    def test_02b_search_logs_poll_reports_job_state(self):
        # Tests [@ANCHOR: pager_duty:search_logs_poll]
        admin = self.env["res.users"].create(
            {
                "name": "Log Poll Admin",
                "login": "log_poll_admin",
                "password": "log_poll_admin",
                "group_ids": [(6, 0, [self.env.ref("pager_duty.group_pager_admin").id])],
            }
        )
        self.env["pager.log.search.job"].create(
            {"uuid": "poll-test-uuid", "state": "done", "result_payload": '{"matches": ["line 1"]}'}
        )
        self.authenticate("log_poll_admin", "log_poll_admin")
        response = self.url_open(
            "/api/v1/pager/logs/search_poll",
            json={"jsonrpc": "2.0", "method": "call", "params": {"job_id": "poll-test-uuid"}},
        )
        result = response.json()["result"]
        self.assertEqual(result["status"], "done")
        self.assertEqual(result["matches"], ["line 1"])

    def test_02c_search_logs_poll_rejects_a_non_admin(self):
        # Tests [@ANCHOR: pager_duty:search_logs_poll]
        self.env["res.users"].create(
            {
                "name": "Plain User",
                "login": "log_poll_plain_user",
                "password": "log_poll_plain_user",
                "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
            }
        )
        self.authenticate("log_poll_plain_user", "log_poll_plain_user")
        response = self.url_open(
            "/api/v1/pager/logs/search_poll",
            json={"jsonrpc": "2.0", "method": "call", "params": {"job_id": "poll-test-uuid"}},
        )
        self.assertIn("error", response.json())

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
        # Tests [@ANCHOR: pager_duty:update_lets_encrypt_domains]
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
