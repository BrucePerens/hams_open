# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestPagerControllers(HamsHttpCase):
    def test_01_ping_endpoint(self):
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
        self.authenticate("tester", "testpassword")
        response_auth = self.url_open("/pager/board")
        self.assertEqual(response_auth.status_code, 200)
