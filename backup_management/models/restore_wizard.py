# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
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

        # Verified by [@ANCHOR: COMM_test_restore_action]
        if not self.env.su and not self.env.user.has_group("backup_management.group_backup_admin"):
            raise AccessError(
                _("Only Backup Administrators can trigger restore operations.")
            )

        if self.snapshot_id.config_id.engine == "kopia":
            validate_backup_path(self.restore_target_path)

        # Additional safety check for pgbackrest stanza
        if self.snapshot_id.config_id.engine == "pgbackrest":
            if not self.restore_target_path:
                raise UserError(_("Restore target stanza is required."))
            validate_backup_path(self.restore_target_path)

        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        jobs = self.env["backup.job"].with_user(svc_uid).with_company(self.snapshot_id.config_id.company_id.id)
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
            # Using list for subprocess ensures no shell injection
            cmd_args = [
                "pgbackrest",
                "restore",
                f"--stanza={self.restore_target_path}",
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
