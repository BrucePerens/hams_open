# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import json
from odoo import models, fields, _
from odoo.exceptions import UserError, AccessError
from .utils import validate_backup_path, publish_to_rabbitmq


class BackupRestoreWizard(models.TransientModel):
    _name = "backup.restore.wizard"
    _description = "Backup Restore Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    snapshot_id = fields.Many2one(
        "backup.snapshot", string="Snapshot", required=True, readonly=True
    )
    restore_target_path = fields.Char(
        string="Restore Directory / Stanza Target",
        required=True,
        help="Path where the backup should be restored, or stanza to target.",
    )

    def action_restore(self):
        self.ensure_one()
        # [@ANCHOR: COMM_backup_trigger_restore]

        # # Verified by [@ANCHOR: COMM_test_restore_action]
        if not self.env.user.has_group("backup_management.group_backup_admin"):
            raise AccessError(
                _("Only Backup Administrators can trigger restore operations.")
            )

        if self.snapshot_id.config_id.engine == "kopia":
            validate_backup_path(self.restore_target_path)

        # Additional safety check for pgbackrest stanza
        if self.snapshot_id.config_id.engine == "pgbackrest":
            if not self.restore_target_path:
                raise UserError(_("Restore target stanza is required."))
            if not self.restore_target_path.replace("_", "").isalnum():
                raise UserError(_("Invalid pgBackRest stanza name. Use only alphanumeric characters and underscores."))

        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        # Bug-hunt fix (2026-09-09, tier-1 pass): do NOT .with_company() to
        # the snapshot's own company here -- see the identical fix and full
        # explanation in backup_config.py's _publish_to_worker. svc_uid's
        # real company_ids is [main_company] only, so .with_company() to any
        # other tenant's company raised AccessError immediately for every
        # restore of a non-main-company backup. backup.job's company_id is
        # a related(store=True) field that computes correctly regardless of
        # env.company; cross-tenant create/read access for this service
        # account now comes from rule_backup_job_multi_tenant_svc instead.
        jobs = self.env["backup.job"].with_user(svc_uid)
        job = jobs.create(
            {
                "config_id": self.snapshot_id.config_id.id,
                "website_id": self.snapshot_id.config_id.website_id.id,
                "job_type": self.snapshot_id.config_id.engine,
                "state": "pending",
                "output_log": "Restore queued in RabbitMQ...",
            }
        )

        cmd_args = []
        if self.snapshot_id.config_id.engine == "kopia":
            cmd_args = [
                "kopia",
                "restore",
                self.snapshot_id.snapshot_id,
                self.restore_target_path,
            ]
        elif self.snapshot_id.config_id.engine == "pgbackrest":
            # Bug-hunt fix (2026-09-09, tier-1 pass): the stanza that gets
            # restored (and therefore which tenant's live PostgreSQL data
            # directory pgbackrest.conf points that stanza at) used to come
            # straight from self.restore_target_path -- a free-text field
            # the ir.rule multi-tenant scoping never touches, because it's
            # typed by the user, not looked up. A backup admin scoped (via
            # rule_backup_config_multi_tenant) to only their own
            # company/website could still select any *stanza name* here,
            # including another tenant's, and combine it with the --set=
            # label from a snapshot they legitimately own -- if that label
            # happens to also exist in the target stanza (backup labels are
            # often just timestamps), this would restore into a different
            # tenant's real database. The stanza a restore targets must
            # always be the same one the snapshot was actually taken from --
            # self.snapshot_id.config_id.target_path, which the ir.rule on
            # backup.snapshot already verified this user may see -- never
            # free text. Using list args for subprocess still ensures no
            # shell injection.
            cmd_args = [
                "pgbackrest",
                "restore",
                f"--stanza={self.snapshot_id.config_id.target_path}",
                f"--set={self.snapshot_id.snapshot_id}",
            ]

        payload = json.dumps(
            {
                "job_id": job.id,
                "config_id": self.snapshot_id.config_id.id,
                "engine": "restore_cmd",
                "cmd_args": cmd_args,
                "snapshot_id": self.snapshot_id.snapshot_id,
                "svc_uid": svc_uid,  # Pass svc_uid for worker to potentially use
                # Needed so the daemon can set KOPIA_PASSWORD for a kopia
                # restore -- previously never included, so every restore of
                # a password-protected Kopia snapshot ran with no password.
                "kopia_password": self.snapshot_id.config_id.kopia_password,
            }
        )

        def publish_task(msg=payload):
            publish_to_rabbitmq(self.env, msg)

        self.env.cr.postcommit.add(publish_task)

        return {
            "type": "ir.actions.act_window",
            "res_model": "backup.job",
            "res_id": job.id,
            "view_mode": "form",
            "target": "current",
        }
