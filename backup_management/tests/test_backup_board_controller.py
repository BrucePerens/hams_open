# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0-or-later License.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestBackupBoardController(HamsHttpCase):
    def test_board_security_and_render(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_board]
        # auth="user": an unauthenticated request must be redirected to login.
        response = self.url_open("/backup/board")
        self.assertTrue(
            "web/login" in response.url,
            'Board endpoint failed to enforce auth="user" security mandate.',
        )

        self.env["res.users"].create(
            {
                "name": "Test Backup Board User",
                "login": "backup_board_tester",
                "password": "backup_board_tester",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.env.flush_all()
        self.authenticate("backup_board_tester", "backup_board_tester")
        response_auth = self.url_open("/backup/board")
        self.assertEqual(response_auth.status_code, 200)
