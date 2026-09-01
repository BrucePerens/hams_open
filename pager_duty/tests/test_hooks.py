# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.hooks import post_init_hook, _claim_info_alias
import logging

_logger = logging.getLogger(__name__)


@tagged("-at_install", "post_install")
class TestPagerDutyHooks(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        
    def test_post_init_hook_daemon_registration(self):
        """
        Test that post_init_hook properly registers the daemon with the admin user
        to avoid Zero-Sudo architecture constraints.
        """


        # Mock register_daemon to ensure it is called with the expected user
        original_register = type(self.env["daemon.key.registry"]).register_daemon
        
        called_with_user_id = None
        
        def mock_register_daemon(registry_self, *args, **kwargs):
            nonlocal called_with_user_id
            called_with_user_id = registry_self.env.user.id
            # Don't actually run it to avoid side effects
            pass
        
        type(self.env["daemon.key.registry"]).register_daemon = mock_register_daemon
        
        try:
            # The test case typically runs with test user or system. 
            # We want to make sure post_init_hook elevates to base.user_admin
            post_init_hook(self.env)
            
            admin_user = self.env.ref("base.user_admin")
            self.assertEqual(
                called_with_user_id,
                admin_user.id,
                "register_daemon should be called with the admin user context."
            )
        finally:
            type(self.env["daemon.key.registry"]).register_daemon = original_register

    def test_claims_info_alias_when_free(self):
        # Tests [@ANCHOR: pager_duty_info_alias_claim]
        # info@hams.com now routes to pager_duty per Bruce's own direction
        # (moved off hams_helpdesk.ticket). The real module install already
        # ran this hook once; simulate a fresh claim by removing whatever it
        # created first.
        self.env["mail.alias"].search([("alias_name", "=", "info")]).unlink()
        _claim_info_alias(self.env)
        alias = self.env["mail.alias"].search([("alias_name", "=", "info")])
        self.assertEqual(len(alias), 1)
        self.assertEqual(
            alias.alias_model_id,
            self.env.ref("pager_duty.model_pager_incident"),
        )

    def test_skips_without_crashing_when_info_already_taken(self):
        # Same collision hams_helpdesk used to work around: stock crm's own
        # default Sales Team also claims "info" (crm/data/crm_team_data.xml),
        # and mail.alias.alias_name is globally unique -- a plain <record>
        # data file would hard-crash this module's entire install the moment
        # crm is present. This reproduces that collision directly against
        # the hook that guards against it, without needing crm installed.
        self.env["mail.alias"].search([("alias_name", "=", "info")]).unlink()
        other_model = self.env.ref("base.model_res_partner")
        self.env["mail.alias"].create(
            {"alias_name": "info", "alias_model_id": other_model.id}
        )
        _claim_info_alias(self.env)  # must not raise
        aliases = self.env["mail.alias"].search([("alias_name", "=", "info")])
        self.assertEqual(len(aliases), 1)
        self.assertEqual(aliases.alias_model_id, other_model)
