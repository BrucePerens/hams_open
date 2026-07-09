# -*- coding: utf-8 -*-
import hashlib
import logging
import os
import shutil
import stat
import tarfile
import tempfile
import urllib.request
import urllib.error
import zipfile
from odoo import models, fields, api, tools, _
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)


class BinaryVersion(models.Model):
    _name = "binary.version"
    _description = "Specific Binary Version Release"
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
        safe_hash = self.checksum[:12] if self.checksum else "nohash"
        filename = f"{self.manifest_id.name}_{self.version_number}_{safe_hash}"
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

        # Skip if perfectly matching binary already exists
        if os.path.exists(target_bin):
            hasher = hashlib.sha256()
            try:
                with open(target_bin, "rb") as f:
                    for chunk in iter(lambda: f.read(4096), b""):
                        hasher.update(chunk)
                if hasher.hexdigest() == self.checksum:
                    if not os.access(target_bin, os.X_OK):
                        os.chmod(target_bin, 0o750)
                    return True
                else:
                    os.unlink(target_bin)
            except OSError as e:
                _logger.warning(
                    "Failed to verify existing central binary %s: %s", target_bin, e
                )

        # Standard secure download and extraction protocol
        try:
            get_req = urllib.request.Request(
                self.url, headers={"User-Agent": "OdooBinaryDownloader/2.0 (Versioned)"}
            )
            tmp_path = None
            try:
                with urllib.request.urlopen(get_req, timeout=600) as response:
                    with tempfile.NamedTemporaryFile(dir=bin_dir, delete=False) as tmp:
                        tmp_path = tmp.name
                        shutil.copyfileobj(response, tmp)

                hasher = hashlib.sha256()
                with open(tmp_path, "rb") as f:
                    for chunk in iter(lambda: f.read(4096), b""):
                        hasher.update(chunk)

                if hasher.hexdigest() != self.checksum:
                    raise UserError(
                        _(
                            "Security Alert: Checksum mismatch for downloaded version %s."
                        )
                        % self.version_number
                    )

                if self.archive_type == "tar.gz":
                    with tarfile.open(  # audit-ignore-path
                        tmp_path, "r:gz"
                    ) as tar:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_version_standard]
                        found = False
                        extract_target = self.extract_member or self.manifest_id.name
                        for member in tar.getmembers():
                            if (
                                member.name.endswith(f"/{extract_target}")
                                or member.name == extract_target
                            ):
                                if member.islnk() or member.issym():
                                    raise UserError(
                                        _(
                                            "Security Alert: Links are not allowed in the archive."
                                        )
                                    )

                                # Path traversal protection
                                member_filename = os.path.basename(member.name)
                                if not member_filename:
                                    continue

                                source = tar.extractfile(member)
                                if source:
                                    with source:
                                        with open(target_bin, "wb") as target:
                                            shutil.copyfileobj(source, target)
                                    found = True
                                    break
                        if not found:
                            raise UserError(
                                _("Member %s not found in archive.") % extract_target
                            )
                elif self.archive_type == "zip":
                    with zipfile.ZipFile(  # audit-ignore-path
                        tmp_path, "r"
                    ) as zip_ref:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_version_standard]
                        extract_target = self.extract_member or self.manifest_id.name
                        found = False
                        for zinfo in zip_ref.infolist():
                            name = zinfo.filename
                            if (
                                name.endswith(f"/{extract_target}")
                                or name == extract_target
                            ):
                                if stat.S_ISLNK(zinfo.external_attr >> 16):
                                    raise UserError(
                                        _(
                                            "Security Alert: Links are not allowed in the archive."
                                        )
                                    )

                                # Path traversal protection
                                member_filename = os.path.basename(zinfo.filename)
                                if not member_filename:
                                    continue

                                with zip_ref.open(zinfo) as source:
                                    with open(target_bin, "wb") as target:
                                        shutil.copyfileobj(source, target)
                                found = True
                                break
                        if not found:
                            raise UserError(
                                _("Member %s not found in zip archive.")
                                % extract_target
                            )
                else:
                    shutil.copy2(tmp_path, target_bin)

                os.chmod(target_bin, 0o750)
                return True
            finally:
                if tmp_path and os.path.exists(tmp_path):
                    try:
                        os.unlink(tmp_path)
                    except OSError as e:
                        _logger.warning(
                            "Failed to clean up temporary file %s: %s", tmp_path, e
                        )
        except (
            urllib.error.URLError,
            OSError,
            tarfile.TarError,
            zipfile.BadZipFile,
            UserError,
        ) as e:
            raise UserError(
                _("Failed to download version %s: %s") % (self.version_number, str(e))
            )
