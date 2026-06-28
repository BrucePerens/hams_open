# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestComplianceSecurity(HamsTransactionCase):

    def test_service_account(self):
        """Verify the compliance service account is correctly configured."""
        svc_user = self.env.ref("compliance.user_compliance_service")
        self.assertTrue(
            svc_user.active,
            "[!] DIAGNOSTIC FOR AI: Service account 'user_compliance_service' should be active. "
            "Check compliance/security/security_data.xml.",
        )
        self.assertTrue(
            svc_user.is_service_account,
            "[!] DIAGNOSTIC FOR AI: User should be marked as a service account (is_service_account=True). "
            "Check compliance/security/security_data.xml.",
        )

        compliance_group = self.env.ref("compliance.group_compliance_service")
        self.assertIn(
            compliance_group,
            svc_user.group_ids,
            "[!] DIAGNOSTIC FOR AI: Service account should belong to the compliance service group. "
            "Check compliance/security/security_data.xml.",
        )

    def test_register_hook_idempotency(self):
        """Verify that _register_hook can be called multiple times safely."""
        # This primarily ensures that the documentation bootstrap doesn't crash
        # when called repeatedly during registry reloads.
        self.env["compliance.config"]._register_hook()
        self.env["compliance.config"]._register_hook()
