# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Refusals that no other test reached: a backup path that climbs back out through '..', a path that cannot be
resolved, an empty path (nothing to validate), and a restore asked for by an administrator who is not a Backup
Administrator."""
import os

from odoo.exceptions import AccessError, UserError
from odoo.tests.common import tagged

from odoo.addons.backup_management.models import utils as backup_utils
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestPathAndRestoreRefusals(HamsTransactionCase):
    def test_a_path_that_uses_dot_dot_is_refused_even_when_it_resolves_inside_a_backup_directory(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_path_validation]
        with self.assertRaisesRegex(UserError, "directory traversal"):
            backup_utils.validate_backup_path("/var/lib/odoo/backups/a/../b")
        backup_utils.validate_backup_path("/var/lib/odoo/backups/a/b")

    def test_a_path_that_cannot_be_resolved_is_refused(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_path_validation]
        with self.safe_patch_object_cm(os.path, "realpath", side_effect=OSError("too many levels of symbolic links")):
            with self.assertRaisesRegex(UserError, "securely resolve"):
                backup_utils.validate_backup_path("/var/lib/odoo/backups/loop")

    def test_an_empty_path_has_nothing_to_validate(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_path_validation]
        self.assertIsNone(backup_utils.validate_backup_path(""))
        self.assertIsNone(backup_utils.validate_backup_path(False))

    def test_a_system_administrator_who_is_not_a_backup_administrator_cannot_restore(self):
        # Tests [@ANCHOR: COMM_backup_trigger_restore]
        user = self.env["res.users"].create({
            "name": "Plain Sysadmin", "login": "plain_sysadmin_restore",
            "group_ids": [(6, 0, [self.env.ref("base.group_system").id])],
        })
        config = self.env["backup.config"].create({
            "name": "Restore Refusal", "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/restore_refusal", "storage_type": "local",
        })
        snapshot = self.env["backup.snapshot"].create({"config_id": config.id, "snapshot_id": "snap_refusal"})
        wizard = self.env["backup.restore.wizard"].with_user(user).create(
            {"snapshot_id": snapshot.id, "restore_target_path": "/var/lib/odoo/backups/restore_out"}
        )
        with self.assertRaisesRegex(AccessError, "Only Backup Administrators"):
            wizard.action_restore()
        self.assertFalse(self.env["backup.job"].search([("config_id", "=", config.id)]))
