# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

@tagged("post_install", "-at_install", "security")
class TestBatch2Security(RealTransactionCase):
    def test_service_account_no_base_user_group(self):
        """
        Verify that the user_backup_service_internal account does NOT have the base.group_user.
        """
        user_backup_service_internal = self.env.ref("backup_management.user_backup_service_internal")
        group_user = self.env.ref("base.group_user")
        
        self.assertNotIn(
            group_user,
            user_backup_service_internal.groups_id,
            "Service account user_backup_service_internal should NOT have base.group_user."
        )
