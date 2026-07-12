# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request


class PagerBoard(http.Controller):
    @http.route("/pager/board", type="http", auth="user", website=True)
    def pager_board(self, **kw):
        # Verified by [@ANCHOR: test_pager_board_url]
        # Safely redirect legacy direct links to the new native Odoo backend Client Action
        action = request.env.ref(
            "pager_duty.action_pager_board_client", raise_if_not_found=False
        )
        if action:
            return request.redirect(f"/odoo?action={action.id}")
        return request.redirect("/odoo")
