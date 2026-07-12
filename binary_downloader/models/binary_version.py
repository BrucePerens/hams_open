# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.
import logging
import os
from odoo import models, fields, api, tools, _
from odoo.exceptions import ValidationError

_logger = logging.getLogger(__name__)


class BinaryVersion(models.Model):
    _inherit = ["binary_downloader.mixin"]
    _name = "binary.version"
    _description = "Specific Binary Version Release"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _order = "release_date desc, id desc"

    manifest_id = fields.Many2one(
        "binary.manifest",
        string="Software Manifest",
        required=True,
        ondelete="cascade",
        index=True,
    )
    version_number = fields.Char(string="Version", required=True)
    release_date = fields.Date(string="Upstream Release Date")
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

    is_downloaded = fields.Boolean(
        string="Downloaded to Central Pool", compute="_compute_is_downloaded"
    )

    _version_uniq = models.Constraint("unique(manifest_id, version_number)", "This version number already exists for this software.")
    _url_not_empty = models.Constraint("CHECK(LENGTH(TRIM(url)) > 0)", "The download URL cannot be empty.")
    _chksum_not_empty = models.Constraint("CHECK(LENGTH(TRIM(checksum)) > 0)", "The checksum cannot be empty.")

    @api.constrains("version_number")
    def _check_version_no_slashes(self):
        for record in self:
            if "/" in record.version_number or "\\" in record.version_number:
                raise ValidationError(
                    _("The version number cannot contain slashes or backslashes.")
                )

    @api.constrains("url")
    def _check_url_scheme(self):
        for record in self:
            if record.url:
                url_trimmed = record.url.strip()
                if not url_trimmed.startswith(("http://", "https://")):
                    raise ValidationError(
                        _(
                            "Only http:// and https:// URLs are allowed for security reasons."
                        )
                    )

    @api.constrains("archive_type", "extract_member")
    def _check_extract_member(self):
        for record in self:
            if record.archive_type in ("tar.gz", "zip") and not record.extract_member:
                raise ValidationError(
                    _("Extract Member is required for %s archives.")
                    % record.archive_type
                )

    def _get_central_path(self):
        """Returns the deterministic central storage path for this specific version."""
        self.ensure_one()
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        filename = self.env["binary_downloader.mixin"]._get_target_filename(self.manifest_id.name, self.checksum)
        return os.path.join(bin_dir, filename)

    @api.depends("version_number", "checksum")
    def _compute_is_downloaded(self):
        for record in self:
            if not record.id or not record.checksum:
                record.is_downloaded = False
                continue
            path = record._get_central_path()
            record.is_downloaded = os.path.exists(path) and os.access(path, os.X_OK)

    def action_download_to_pool(self):
        # [@ANCHOR: binary_version_download_pool]
        """Downloads and verifies the binary into the central version pool."""
        self.ensure_one()

        # Deterministic advisory lock to prevent concurrent downloads of the SAME version
        lock_id = self.env["zero_sudo.security.utils"]._get_deterministic_hash(
            f"binary_version_install_{self.manifest_id.name}_{self.version_number}_{self.checksum}"
        )
        self.env.cr.execute("SELECT pg_advisory_xact_lock(%s)", (lock_id,))

        target_bin = self._get_central_path()
        bin_dir = os.path.dirname(target_bin)

        if not os.path.exists(bin_dir):
            os.makedirs(bin_dir, exist_ok=True)
            os.chmod(bin_dir, 0o750)



    def action_notify_tenants(self):
        self.ensure_one()
        limit = 100
        offset = 0
        LinkModel = self.env["binary.tenant.link"]
        IncidentModel = self.env["pager.incident"]

        while True:
            links = LinkModel.search(
                [("manifest_id", "=", self.manifest_id.id), ("active_version_id", "!=", self.id)],
                limit=limit,
                offset=offset,
            )
            if not links:
                break
                
            incident_vals = []
            for link in links:
                incident_vals.append({
                    "source": "binary_update",
                    "severity": "medium",
                    "description": _("New binary version %s is available for %s.") % (self.version_number, self.manifest_id.name),
                    "website_id": link.website_id.id,
                })
                
            if incident_vals:
                IncidentModel.create(incident_vals)
                
            offset += limit
            
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Tenants notified via PagerDuty."),
                "type": "success",
            },
        }

    def unlink(self):
        for record in self:
            if record.manifest_id.name and record.checksum:
                self.env["binary_downloader.mixin"]._unlink_binary_file(record.manifest_id.name, record.checksum)
        return super().unlink()
