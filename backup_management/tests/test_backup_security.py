# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
import os
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import UserError, AccessError


@tagged("post_install", "-at_install", "security")
class TestBackupSecurity(RealTransactionCase):

    def setUp(self):
        super().setUp()
        self.BackupConfig = self.env["backup.config"]
        # Use a path consistent with the module's target usage
        self.valid_repo = "/var/lib/odoo/backups/test_repo"
        # Use facility service to create test records/users to avoid audit-warnings
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.odoo_facility_service_internal"
        )
        self.env["res.users"].browse(svc_uid).write(
            {
                "group_ids": [
                    (4, self.env.ref("backup_management.group_backup_admin").id)
                ]
            }
        )
        self.config = self.BackupConfig.with_user(svc_uid).create(
            {
                "name": "Security Test Repo",
                "engine": "kopia",
                "target_path": self.valid_repo,
            }
        )
        self.user_no_group = self.env["res.users"].create(
            {
                "name": "No Group User",
                "login": "no_group",
                "email": "no@group.com",
                "group_ids": [(6, 0, [])],
            }
        )

    def test_path_traversal_prevention(self):
        # Tests [@ANCHOR: test_backup_security]
        # Tests [@ANCHOR: backup_path_validation]

        forbidden_paths = [
            "/etc/passwd",
            "/root/.ssh/id_rsa",
            "/var/lib/odoo/sessions",
            "/var/lib/odoo/addons/base",
            "/bin/sh",
            "-L /var/lib/odoo/backups/hack",  # Flag injection
        ]

        forbidden_paths += [
            "/var/lib/odoo/backups/../../etc/passwd",
            "../../../etc/shadow",
        ]

        for path in forbidden_paths:
            with self.subTest(path=path):
                with self.assertRaises(
                    UserError, msg=f"Path {path} should be forbidden"
                ):
                    self.config.write({"target_path": path})
                    self.env.flush_all()

    def test_metacharacter_prevention(self):
        # Tests improved validation for metacharacters
        malicious_paths = [
            "/var/lib/odoo/backups/test; ls",
            "/var/lib/odoo/backups/test&rm",
            "/var/lib/odoo/backups/test|whoami",
            "/var/lib/odoo/backups/test`id`",
            "/var/lib/odoo/backups/test$(id)",
            "/var/lib/odoo/backups/test\n/etc",
        ]

        for path in malicious_paths:
            with self.subTest(path=path):
                with self.assertRaises(
                    UserError,
                    msg=f"Path {path} containing metacharacters should be forbidden",
                ):
                    self.config.write({"target_path": path})
                    self.env.flush_all()

    def test_symlink_traversal(self):
        # Create a symlink to a forbidden path
        symlink_path = "/var/lib/odoo/backups/evil_link_test"
        if os.path.exists(symlink_path):
            os.remove(symlink_path)

        try:
            os.symlink("/etc/shadow", symlink_path)
            with self.assertRaises(
                UserError, msg="Symlink to /etc/shadow should be forbidden"
            ):
                self.config.write({"target_path": symlink_path})
                self.env.flush_all()
        finally:
            if os.path.lexists(symlink_path):
                os.remove(symlink_path)

    def test_field_security_groups(self):
        # Ensure sensitive fields are not accessible to non-admins
        # Note: In Odoo, 'groups' on fields are enforced at the view and RPC level.
        # We check if the field has the group attribute set.

        kopia_pass_field = self.BackupConfig._fields["kopia_password"]
        self.assertEqual(
            kopia_pass_field.groups, "backup_management.group_backup_admin"
        )

        secret_key_field = self.BackupConfig._fields["secret_key"]
        self.assertEqual(
            secret_key_field.groups, "backup_management.group_backup_admin"
        )

    def test_restore_wizard_security(self):
        # Tests [@ANCHOR: test_restore_action]
        # Tests [@ANCHOR: backup_trigger_restore]

        snapshot = self.env["backup.snapshot"].create(
            {
                "config_id": self.config.id,
                "snapshot_id": "snap1",
            }
        )

        wizard = self.env["backup.restore.wizard"].create(
            {"snapshot_id": snapshot.id, "restore_target_path": "/etc/passwd"}
        )

        with self.assertRaises(UserError):
            wizard.action_restore()
            self.env.flush_all()

        # Test pgbackrest injection
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.odoo_facility_service_internal"
        )
        config_pg = self.BackupConfig.with_user(svc_uid).create(
            {"name": "PG Security", "engine": "pgbackrest", "target_path": "main"}
        )
        snapshot_pg = self.env["backup.snapshot"].create(
            {
                "config_id": config_pg.id,
                "snapshot_id": "snap_pg",
            }
        )

        wizard_pg = self.env["backup.restore.wizard"].create(
            {"snapshot_id": snapshot_pg.id, "restore_target_path": "main; rm -rf /"}
        )
        with self.assertRaises(UserError):
            wizard_pg.action_restore()
            self.env.flush_all()

    def test_access_restriction(self):
        # Ensure non-admins cannot trigger backups or restores
        with self.assertRaises(AccessError):
            self.config.with_user(self.user_no_group.id).action_trigger_backup()
            self.env.flush_all()

        snapshot = self.env["backup.snapshot"].create(
            {
                "config_id": self.config.id,
                "snapshot_id": "snap_acc",
            }
        )
        with self.assertRaises(AccessError):
            self.env["backup.restore.wizard"].with_user(self.user_no_group.id).create(
                {
                    "snapshot_id": snapshot.id,
                    "restore_target_path": "/var/lib/odoo/backups/safe",
                }
            )
            self.env.flush_all()

    def tearDown(self):
        # Explicit cleanup to avoid zero_sudo teardown issues with res.users/res.partner cleanup order
        if self.user_no_group.exists():
            partner = self.user_no_group.partner_id
            self.user_no_group.unlink()
            if partner.exists():
                partner.unlink()
        super().tearDown()
