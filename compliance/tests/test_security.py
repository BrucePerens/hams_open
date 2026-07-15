# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General
# Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestComplianceSecurity(HamsTransactionCase):

    def test_service_account(self):
        """Verify the compliance service account is correctly configured."""
        svc_user = self.env.ref("compliance.user_compliance_service")
        msg1 = (
            "[!] DIAGNOSTIC FOR AI: Service account 'user_compliance_service' should be active. "
            "Check compliance/security/security_data.xml."
        )
        self.assertTrue(svc_user.active, msg1)
        msg2 = (
            "[!] DIAGNOSTIC FOR AI: User should be marked as a service account (is_service_account=True). "
            "Check compliance/security/security_data.xml."
        )
        self.assertTrue(svc_user.is_service_account, msg2)

        compliance_group = self.env.ref("compliance.group_compliance_service")
        msg3 = (
            "[!] DIAGNOSTIC FOR AI: Service account should belong to the compliance service group. "
            "Check compliance/security/security_data.xml."
        )
        self.assertIn(compliance_group, svc_user.group_ids, msg3)
