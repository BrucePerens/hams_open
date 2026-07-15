# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging
import os
from odoo import models, fields, api, tools, _

_logger = logging.getLogger(__name__)


class BinaryTenantLink(models.Model):
    _inherit = ["mail.thread", "mail.activity.mixin"]
    _name = "binary.tenant.link"
    _description = "Tenant to Binary Version Assignment"
    name = fields.Char(string="Name", default=lambda self: self._description)

    website_id = fields.Many2one(
        "website",
        string="Tenant / Website",
        required=True,
        ondelete="cascade",
        index=True,
    )
    manifest_id = fields.Many2one(
        "binary.manifest",
        string="Software",
        required=True,
        ondelete="cascade",
        index=True,
    )
    active_version_id = fields.Many2one(
        "binary.version",
        string="Active Version",
        required=True,
        domain="[('manifest_id', '=', manifest_id)]",
        index=True,
    )
    symlink_path = fields.Char(
        string="Tenant Execution Path", compute="_compute_symlink_path"
    )

    _tenant_manifest_uniq = models.Constraint("unique(website_id, manifest_id)", "A tenant can only have one active version of a specific binary at a time.")

    @api.depends("website_id", "manifest_id")
    def _compute_symlink_path(self):
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        for record in self:
            if not record.website_id or not record.manifest_id:
                record.symlink_path = False
                continue
            # Isolate tenant binaries in their own namespace
            tenant_dir = os.path.join(
                data_dir, "tenant_bins", f"site_{record.website_id.id}"
            )
            if ".." in record.manifest_id.name.split(os.path.sep) or "/" in record.manifest_id.name:
                record.symlink_path = False
                continue
            record.symlink_path = os.path.join(tenant_dir, record.manifest_id.name)

    def apply_symlink(self):
        """Creates or updates the OS-level symlink pointing to the central version pool."""
        self.ensure_one()
        if not self.active_version_id.is_downloaded:
            self.active_version_id.action_download_to_pool()

        if not self.symlink_path:
            return False

        central_target = self.active_version_id._get_central_path()
        link_path = self.symlink_path
        tenant_dir = os.path.dirname(link_path)

        # [@ANCHOR: pure_python_symlink_engine]
        if not os.path.exists(tenant_dir):
            os.makedirs(tenant_dir, exist_ok=True)
            os.chmod(tenant_dir, 0o750)

        if os.path.lexists(link_path):
            current_target = (
                os.readlink(link_path) if os.path.islink(link_path) else None
            )
            if current_target == central_target:
                return True  # Link is already correct
            os.unlink(link_path)  # audit-ignore-path  # fmt: skip

        os.symlink(central_target, link_path)  # audit-ignore-path  # fmt: skip
        _logger.info("Created tenant symlink: %s -> %s", link_path, central_target)
        return True

    @api.model_create_multi
    def create(self, vals_list):
        records = super().create(vals_list)
        for record in records:
            record.apply_symlink()
        return records

    def write(self, vals):
        res = super().write(vals)
        if "active_version_id" in vals:
            for record in self:
                record.apply_symlink()
        return res

    def unlink(self):
        for record in self:
            if record.symlink_path and os.path.lexists(record.symlink_path):
                try:
                    os.unlink(record.symlink_path)  # audit-ignore-path  # fmt: skip
                except OSError as e:
                    _logger.warning(
                        "Failed to clean up tenant symlink %s: %s",
                        record.symlink_path,
                        e,
                    )
        return super().unlink()

    def action_upgrade_to_latest(self):
        """Finds the most recent upstream release and automatically repoints the tenant symlink."""
        self.ensure_one()
        latest = self.env["binary.version"].search(
            [("manifest_id", "=", self.manifest_id.id)],
            order="release_date desc, id desc",
            limit=1,
        )

        if not latest:
            return False

        if latest.id == self.active_version_id.id:
            msg = _("The tenant is already running the latest binary version.")
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Up to Date"),
                    "message": msg,
                    "type": "info",
                },
            }

        self.active_version_id = latest.id

        msg = _("Tenant execution path successfully symlinked to version %s.")
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Upgrade Successful"),
                "message": msg % latest.version_number,
                "type": "success",
                "next": {"type": "ir.actions.client", "tag": "reload"},
            },
        }
