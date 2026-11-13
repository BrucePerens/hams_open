# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestPagerMenuVisibility(HamsTransactionCase):
    """The NOC dashboard and log search are client actions, so Odoo does not hide them for a user who
    cannot read the models behind them: the root menu itself must carry the Pager Duty Admin group."""

    def test_pager_menus_hidden_from_plain_internal_user(self):
        user = self.env["res.users"].create({
            "name": "Plain internal", "login": "plain_internal_pager",
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })
        visible = self.env["ir.ui.menu"].with_user(user).load_menus(False)
        for xmlid in ("menu_admin_root", "menu_pager_board", "menu_pager_log_viewer"):
            menu = self.env.ref("pager_duty." + xmlid)
            self.assertNotIn(menu.id, visible, xmlid)

    def test_pager_menus_visible_to_pager_admin(self):
        admin = self.env.ref("base.user_admin")
        visible = self.env["ir.ui.menu"].with_user(admin).load_menus(False)
        for xmlid in ("menu_admin_root", "menu_pager_board", "menu_pager_log_viewer"):
            self.assertIn(self.env.ref("pager_duty." + xmlid).id, visible, xmlid)
