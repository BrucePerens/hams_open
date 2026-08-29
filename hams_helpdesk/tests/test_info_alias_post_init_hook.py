# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

from odoo.addons.hams_helpdesk import post_init_hook


@tagged("post_install", "-at_install")
class TestInfoAliasPostInitHook(HamsTransactionCase):
    def test_01_claims_info_alias_when_free(self):
        # The real module install already ran this hook once; simulate a
        # fresh claim by removing whatever it created first.
        self.env["mail.alias"].search([("alias_name", "=", "info")]).unlink()
        post_init_hook(self.env)
        alias = self.env["mail.alias"].search([("alias_name", "=", "info")])
        self.assertEqual(len(alias), 1)
        self.assertEqual(
            alias.alias_model_id,
            self.env.ref("hams_helpdesk.model_hams_helpdesk_ticket"),
        )

    def test_02_skips_without_crashing_when_info_already_taken(self):
        # Found live 2026-08-29 installing this module alongside crm for the
        # first time: crm's own default Sales Team also claims "info"
        # (crm/data/crm_team_data.xml), and mail.alias.alias_name is globally
        # unique -- the old plain <record> data file hard-crashed this
        # module's entire install the moment crm was present. This
        # reproduces that collision directly against the hook that replaced
        # it, without needing crm actually installed.
        self.env["mail.alias"].search([("alias_name", "=", "info")]).unlink()
        other_model = self.env.ref("base.model_res_partner")
        self.env["mail.alias"].create(
            {"alias_name": "info", "alias_model_id": other_model.id}
        )
        post_init_hook(self.env)  # must not raise
        aliases = self.env["mail.alias"].search([("alias_name", "=", "info")])
        self.assertEqual(len(aliases), 1)
        self.assertEqual(aliases.alias_model_id, other_model)
