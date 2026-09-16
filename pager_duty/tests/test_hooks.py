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
        # [@ANCHOR: pager_duty:test_post_init_hook_registers_as_own_service_account]
        """post_init_hook registers pager_duty's daemon key as its own service
        account, never as base.user_admin (zero-sudo, least privilege).

        This test used to assert the opposite -- that the hook elevated to
        base.user_admin -- and mocked register_daemon out entirely, so it
        could not show that a narrower identity would even be authorized.
        Now the real register_daemon() runs, including its authorization
        check (a service account may provision a key only for itself); only
        the final key rotation, which writes under /opt/hams/etc/keys, is
        replaced.
        """
        registry_cls = type(self.env["daemon.key.registry"])
        original_register = registry_cls.register_daemon
        caller_uids = []

        def spy_register_daemon(registry_self, *args, **kwargs):
            caller_uids.append(registry_self.env.uid)
            return original_register(registry_self, *args, **kwargs)

        self.safe_patch_object(registry_cls, "register_daemon", new=spy_register_daemon)
        rotate = self.safe_patch_object(registry_cls, "_rotate_key_and_write_file")

        post_init_hook(self.env)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        self.assertEqual(
            caller_uids,
            [svc_uid],
            "[!] DIAGNOSTIC FOR AI: pager_duty must register its daemon key as its "
            "own service account.",
        )
        self.assertNotIn(self.env.ref("base.user_admin").id, caller_uids)
        self.assertTrue(rotate.called, "register_daemon must reach key rotation.")
        registry = self.env["daemon.key.registry"].search(
            [("name", "=", "Pager Duty - Generalized Monitor")]
        )
        self.assertEqual(registry.user_id.id, svc_uid)

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
