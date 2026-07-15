# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import http
from odoo.http import request


class BackupBoard(http.Controller):
    @http.route("/backup/board", type="http", auth="user", website=True)
    def backup_board(self):
        action = request.env.ref(
            "backup_management.action_backup_board_client", raise_if_not_found=False
        )
        if action:
            return request.redirect(f"/odoo?action={action.id}")
        return request.redirect("/odoo")
