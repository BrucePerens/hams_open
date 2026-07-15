# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging
import os
import shutil
from odoo import models, fields, api, tools, _
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)


class BinaryManifest(models.Model):
    _inherit = ["binary_downloader.mixin"]
    _name = "binary.manifest"
    _description = "Binary Download Manifest"
    # # Verified by [@ANCHOR: COMM_test_binary_manifest_views]

    name = fields.Char(
        string="Binary Name", required=True, help="Command name (e.g., kopia)"
    )
    url = fields.Char(string="Download URL", required=True)
    checksum = fields.Char(string="SHA-256 Checksum", required=True)
    archive_type = fields.Selection(
        [
            ("binary", "Raw Binary"),
            ("tar.gz", "Tarball (.tar.gz)"),
            ("zip", "Zip Archive (.zip)"),
        ],
        string="Archive Type",
        default="binary",
        required=True,
    )
    extract_member = fields.Char(
        string="Extract Member", help="Specific file to extract from the archive."
    )
    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=False,
        default=lambda self: self.env.company,
        index=True,
    )

    @api.constrains("url")
    def _check_url_scheme(self):
        for record in self:
            if record.url:
                url_trimmed = record.url.strip()
                if not url_trimmed.startswith("https://"):
                    msg = _("""Only https:// URLs are allowed for security reasons.""")
                    raise ValidationError(msg)

    is_installed = fields.Boolean(string="Installed", compute="_compute_is_installed")

    _msg_name_uniq = """The binary name must be unique per company!"""
    _name_uniq = models.Constraint("UNIQUE(name, company_id)", _msg_name_uniq)
    _msg_name_not_empty = """The binary name cannot be empty."""
    _name_not_empty = models.Constraint("CHECK(LENGTH(TRIM(name)) > 0)", _msg_name_not_empty)
    _msg_url_not_empty = """The download URL cannot be empty."""
    _url_not_empty = models.Constraint("CHECK(LENGTH(TRIM(url)) > 0)", _msg_url_not_empty)
    _msg_chksum_not_empty = """The checksum cannot be empty."""
    _chksum_not_empty = models.Constraint("CHECK(LENGTH(TRIM(checksum)) > 0)", _msg_chksum_not_empty)

    @api.constrains("name")
    def _check_name_no_slashes(self):
        for record in self:
            if "/" in record.name or "\\" in record.name:
                msg = _("""The binary name cannot contain slashes or backslashes.""")
                raise ValidationError(msg)
            if record.name in (".", ".."):
                raise ValidationError(_("The binary name cannot be '.' or '..'."))

    @api.constrains("archive_type", "extract_member")
    def _check_extract_member(self):
        for record in self:
            if record.archive_type in ("tar.gz", "zip") and not record.extract_member:
                msg = _("""Extract Member is required for %s archives.""")
                raise ValidationError(msg % record.archive_type)

    @api.depends("name", "checksum", "archive_type", "extract_member")
    def _compute_is_installed(self):
        # [@ANCHOR: binary_compute_installed]

        # # Verified by [@ANCHOR: test_binary_manifest_standard]
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        for record in self:
            if not record.name:
                record.is_installed = False
                continue

            if shutil.which(record.name):
                record.is_installed = True
                continue

            # Check hams_bin
            filename = self.env["binary_downloader.mixin"]._get_target_filename(record.name, record.checksum)
            target_bin = os.path.join(bin_dir, filename)
            if os.path.exists(target_bin) and os.access(target_bin, os.X_OK):
                record.is_installed = True
            else:
                record.is_installed = False

    def action_install(self):
        # [@ANCHOR: binary_action_install]

        # # Verified by [@ANCHOR: test_binary_manifest_standard]
        self.ensure_one()
        # Security: ensure only users with appropriate groups can trigger this
        if not (
            self.env.user.has_group("binary_downloader.group_binary_downloader_manager")
            or self.env.is_admin()
        ):
            msg = _("""You do not have sufficient permissions to install binaries.""")
            raise UserError(msg)

        self.ensure_executable(self.name)

        msg_success = _("""Installation of %s completed successfully.""")
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": msg_success % self.name,
                "sticky": False,
                "type": "success",
                "next": {"type": "ir.actions.client", "tag": "reload"},
            },
        }

    @api.model
    def ensure_executable(self, cmd_name):
        # [@ANCHOR: binary_ensure_executable]

        # [@ANCHOR: COMM_binary_resolution]

        # # Verified by [@ANCHOR: test_binary_manifest_standard]
        if (
            not cmd_name
            or "/" in cmd_name
            or "\\" in cmd_name
            or cmd_name in (".", "..")
        ):
            raise ValidationError(_("Invalid binary name: %s") % cmd_name)

        # CONDUCT SECURITY AUDIT: Binary manifests are technical system-wide resources.
        # We use the micro-privilege service account for resolution.
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "binary_downloader.user_binary_downloader_service"
        )

        # Strict Multi-Tenant Resolution: Current Company -> Global Fallback
        manifest_records = (
            self.env["binary.manifest"]
            .with_user(svc_uid)
            .search([
                ("name", "=", cmd_name), 
                ("company_id", "in", [self.env.company.id, False])
            ], limit=2)
        )
        manifest_record = manifest_records.filtered(lambda r: r.company_id.id == self.env.company.id)
        if not manifest_record:
            manifest_record = manifest_records.filtered(lambda r: not r.company_id)

        if not manifest_record:
            msg = _("""Missing dependency: '%s'. Please configure it in Settings -> Technical -> Binary Manifests or install via OS package manager.""")
            raise UserError(msg % cmd_name)


        return self.env["binary_downloader.mixin"].with_user(svc_uid)._download_and_extract(
            cmd_name=manifest_record.name,
            url=manifest_record.url,
            checksum=manifest_record.checksum,
            archive_type=manifest_record.archive_type,
            extract_member=manifest_record.extract_member,
        )


    def unlink(self):
        checksums = [r.checksum for r in self if r.checksum]
        checksum_counts = {}
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "binary_downloader.user_binary_downloader_service"
        )
        if checksums:
            manifest_groups = self.env["binary.manifest"].with_user(svc_uid)._read_group(
                [("checksum", "in", checksums)],
                groupby=["checksum"],
                aggregates=["__count"]
            )
            for checksum, count in manifest_groups:
                checksum_counts[checksum] = count

            version_groups = self.env["binary.version"].with_user(svc_uid)._read_group(
                [("checksum", "in", checksums)],
                groupby=["checksum"],
                aggregates=["__count"]
            )
            for checksum, count in version_groups:
                checksum_counts[checksum] = checksum_counts.get(checksum, 0) + count


        for record in self:
            if record.name and record.checksum:
                if checksum_counts.get(record.checksum, 0) <= 1:
                    self.env["binary_downloader.mixin"].with_user(svc_uid)._unlink_binary_file(record.name, record.checksum)
        return super().unlink()
