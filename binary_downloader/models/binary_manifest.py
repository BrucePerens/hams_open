# -*- coding: utf-8 -*-
import hashlib
import logging
import os
import platform
import shutil
import stat
import tarfile
import zipfile
import tempfile
import urllib.request
import urllib.error
from odoo import models, fields, api, tools, _
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)


class BinaryManifest(models.Model):
    _name = "binary.manifest"
    _description = "Binary Download Manifest"

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
                if not url_trimmed.startswith(("http://", "https://")):
                    raise ValidationError(
                        _(
                            "Only http:// and https:// URLs are allowed for security reasons."
                        )
                    )

    is_installed = fields.Boolean(string="Installed", compute="_compute_is_installed")

    _name_uniq = models.Constraint(
        "UNIQUE(name, company_id)", "The binary name must be unique per company!"
    )
    _name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "The binary name cannot be empty."
    )
    _url_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(url)) > 0)", "The download URL cannot be empty."
    )
    _chksum_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(checksum)) > 0)", "The checksum cannot be empty."
    )

    @api.constrains("name")
    def _check_name_no_slashes(self):
        for record in self:
            if "/" in record.name or "\\" in record.name:
                raise ValidationError(
                    _("The binary name cannot contain slashes or backslashes.")
                )
            if record.name in (".", ".."):
                raise ValidationError(_("The binary name cannot be '.' or '..'."))

    @api.constrains("archive_type", "extract_member")
    def _check_extract_member(self):
        for record in self:
            if record.archive_type in ("tar.gz", "zip") and not record.extract_member:
                raise ValidationError(
                    _("Extract Member is required for %s archives.")
                    % record.archive_type
                )

    @api.depends("name")
    def _compute_is_installed(self):
        # [@ANCHOR: binary_compute_installed]
        # Verified by [@ANCHOR: test_binary_manifest_standard]
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        for record in self:
            if not record.name:
                record.is_installed = False
                continue

            # Check system path
            if shutil.which(record.name):
                record.is_installed = True
                continue

            # Check hams_bin
            filename = record._get_target_filename()
            target_bin = os.path.join(bin_dir, filename)
            if os.path.exists(target_bin) and os.access(target_bin, os.X_OK):
                record.is_installed = True
            else:
                record.is_installed = False

    def _get_target_filename(self):
        """Generates a stable, unique filename based on the binary name and its checksum.
        This allows multiple versions or company-specific variants of the same command
        to coexist in the shared hams_bin directory.
        """
        self.ensure_one()
        # Hash name and checksum to create a unique identifier for this specific binary variant
        identifier = hashlib.sha256(
            f"{self.name}_{self.checksum}".encode()
        ).hexdigest()[:16]
        return f"{self.name}_{identifier}"

    def action_install(self):
        # [@ANCHOR: binary_action_install]
        # Verified by [@ANCHOR: test_binary_manifest_standard]
        self.ensure_one()
        # Security: ensure only users with appropriate groups can trigger this
        if not (
            self.env.user.has_group("binary_downloader.group_binary_downloader_manager")
            or self.env.is_admin()
        ):
            raise UserError(
                _("You do not have sufficient permissions to install binaries.")
            )

        self.ensure_executable(self.name)
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Successfully installed %s.") % self.name,
                "sticky": False,
                "type": "success",
            },
        }

    @api.model
    def ensure_executable(self, cmd_name):
        # [@ANCHOR: binary_ensure_executable]
        # [@ANCHOR: binary_resolution]
        # Verified by [@ANCHOR: test_binary_manifest_standard]
        if (
            not cmd_name
            or "/" in cmd_name
            or "\\" in cmd_name
            or cmd_name in (".", "..")
        ):
            raise ValidationError(_("Invalid binary name: %s") % cmd_name)

        path = shutil.which(cmd_name)
        if path:
            return path

        # CONDUCT SECURITY AUDIT: Binary manifests are technical system-wide resources.
        # We use the micro-privilege service account for resolution.
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "binary_downloader.user_binary_downloader_service"
        )

        # Strict Multi-Tenant Resolution: Current Company -> Global Fallback
        manifest_record = (
            self.env["binary.manifest"]
            .with_user(svc_uid)
            .search(
                [("name", "=", cmd_name), ("company_id", "=", self.env.company.id)],
                limit=1,
            )
        )
        if not manifest_record:
            manifest_record = (
                self.env["binary.manifest"]
                .with_user(svc_uid)
                .search([("name", "=", cmd_name), ("company_id", "=", False)], limit=1)
            )

        if not manifest_record:
            raise UserError(
                _(
                    "Missing dependency: '%s'. Please configure it in Settings -> Technical -> Binary Manifests or install via OS package manager."
                )
                % cmd_name
            )

        if platform.system() != "Linux" or platform.machine() != "x86_64":
            raise UserError(
                _(
                    "Auto-install of %s is only supported on Linux x86_64. Please install manually."
                )
                % cmd_name
            )

        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")

        if not os.path.exists(bin_dir):
            os.makedirs(bin_dir, exist_ok=True)
            os.chmod(bin_dir, 0o750)

        target_bin = os.path.join(bin_dir, manifest_record._get_target_filename())

        # Deterministic advisory lock to prevent concurrent downloads of the SAME binary
        lock_id = self.env["zero_sudo.security.utils"]._get_deterministic_hash(
            f"binary_install_{cmd_name}_{manifest_record.checksum}"
        )
        self.env.cr.execute("SELECT pg_advisory_xact_lock(%s)", (lock_id,))

        if os.path.exists(target_bin):
            # Checksum verification for existing binary
            hasher = hashlib.sha256()
            try:
                with open(
                    target_bin, "rb"
                ) as f:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                    for chunk in iter(lambda: f.read(4096), b""):
                        hasher.update(chunk)
                if hasher.hexdigest() == manifest_record.checksum:
                    # [!] DIAGNOSTIC FOR AI: Checksum verified for existing binary.
                    if not os.access(target_bin, os.X_OK):
                        os.chmod(target_bin, 0o750)
                    return target_bin
                else:
                    _logger.info(
                        "Checksum mismatch for %s, re-downloading...", cmd_name
                    )
                    os.unlink(
                        target_bin
                    )  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
            except OSError as e:
                _logger.warning("Failed to check existing binary %s: %s", target_bin, e)

        try:
            get_req = urllib.request.Request(
                manifest_record.url, headers={"User-Agent": "OdooBinaryDownloader/1.0"}
            )
            tmp_path = None
            try:
                with urllib.request.urlopen(get_req, timeout=600) as response:
                    etag = response.getheader("ETag")
                    if etag:
                        _logger.info("Download successful, ETag: %s", etag)

                    with tempfile.NamedTemporaryFile(dir=bin_dir, delete=False) as tmp:
                        tmp_path = tmp.name
                        shutil.copyfileobj(response, tmp)

                hasher = hashlib.sha256()
                with open(
                    tmp_path, "rb"
                ) as f:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                    for chunk in iter(lambda: f.read(4096), b""):
                        hasher.update(chunk)

                if hasher.hexdigest() != manifest_record.checksum:
                    # [!] DIAGNOSTIC FOR AI: Downloaded checksum mismatch. Expected %s, got %s
                    _logger.error(
                        "Checksum mismatch for %s. Expected %s, got %s",
                        cmd_name,
                        manifest_record.checksum,
                        hasher.hexdigest(),
                    )
                    raise UserError(
                        _("Security Alert: Checksum mismatch for downloaded %s binary.")
                        % cmd_name
                    )

                if manifest_record.archive_type == "tar.gz":
                    with tarfile.open(
                        tmp_path, "r:gz"
                    ) as tar:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                        found = False
                        extract_target = manifest_record.extract_member or cmd_name
                        for member in tar.getmembers():
                            if (
                                member.name.endswith(f"/{extract_target}")
                                or member.name == extract_target
                            ):
                                # Deep link/symlink protection
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
                                        with open(
                                            target_bin, "wb"
                                        ) as target:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                                            shutil.copyfileobj(source, target)
                                    found = True
                                    break
                        if not found:
                            raise UserError(
                                _("Member %s not found in archive.") % extract_target
                            )
                elif manifest_record.archive_type == "zip":
                    with zipfile.ZipFile(
                        tmp_path, "r"
                    ) as zip_ref:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                        extract_target = manifest_record.extract_member or cmd_name
                        found = False
                        for zinfo in zip_ref.infolist():
                            name = zinfo.filename
                            if (
                                name.endswith(f"/{extract_target}")
                                or name == extract_target
                            ):
                                # Security: Check for symlinks (external attributes)
                                # ZIP external attributes: bits 16-31 for Unix permissions
                                if (zinfo.external_attr >> 16) & stat.S_IFLNK:
                                    raise UserError(
                                        _(
                                            "Security Alert: Links are not allowed in the archive."
                                        )
                                    )

                                # Path traversal protection for ZIP
                                member_filename = os.path.basename(zinfo.filename)
                                if not member_filename:
                                    continue

                                with zip_ref.open(
                                    zinfo
                                ) as source:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                                    with open(
                                        target_bin, "wb"
                                    ) as target:  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
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
                return target_bin
            finally:
                if tmp_path and os.path.exists(tmp_path):
                    try:
                        os.unlink(
                            tmp_path
                        )  # audit-ignore-path: Tested by [@ANCHOR: test_binary_manifest_standard]
                    except OSError as e:
                        _logger.warning(
                            "Failed to remove temporary file %s: %s", tmp_path, e
                        )
        except (UserError, ValidationError):
            raise
        except (
            urllib.error.URLError,
            OSError,
            tarfile.TarError,
            zipfile.BadZipFile,
        ) as e:
            _logger.exception("Failed to auto-install %s", cmd_name)
            raise UserError(_("Failed to auto-install %s: %s") % (cmd_name, str(e)))
