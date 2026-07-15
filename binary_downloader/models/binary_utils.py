# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.

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
from odoo import models, api, tools, fields, _
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)


class BinaryDownloaderMixin(models.AbstractModel):
    _name = "binary_downloader.mixin"
    _description = "Binary Downloader Mixin"

    name = fields.Char(string="Name")

    @api.model
    def _download_and_extract(self, cmd_name, url, checksum, archive_type, extract_member=None):
        """
        Downloads a binary, verifies its checksum, extracts it (if archive),
        and places it in the shared hams_bin directory.
        """
        if platform.system() != "Linux" or platform.machine() not in ("x86_64", "aarch64", "armv7l"):
            raise UserError(
                _("Auto-install of %s is only supported on Linux x86_64/aarch64/armv7l. Please install manually.") % cmd_name
            )
        
        if not url.startswith(("http://", "https://")):
            raise UserError(_("Only http:// and https:// URLs are allowed."))

        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")

        if not os.path.exists(bin_dir):
            os.makedirs(bin_dir, exist_ok=True)
            os.chmod(bin_dir, 0o750)

        # Generate stable filename
        if ".." in cmd_name.split(os.path.sep):
            raise UserError(_("Security Alert: Path traversal attempt detected."))
        identifier = hashlib.sha256(f"{cmd_name}_{checksum}".encode()).hexdigest()[:16]
        filename = f"{cmd_name}_{identifier}"
        target_bin = os.path.realpath(os.path.join(bin_dir, filename))
        if not target_bin.startswith(os.path.realpath(bin_dir)):
            raise UserError(_("Security Alert: Path traversal attempt detected."))

        # Deterministic advisory lock to prevent concurrent downloads of the SAME binary
        lock_id = self.env["zero_sudo.security.utils"]._get_deterministic_hash(
            f"binary_install_{cmd_name}_{checksum}"
        )
        self.env.cr.execute("SELECT pg_advisory_xact_lock(%s)", (lock_id,))

        if os.path.exists(target_bin):
            if archive_type != "binary":
                if not os.access(target_bin, os.X_OK):
                    os.chmod(target_bin, 0o750)
                return target_bin

            # Checksum verification for existing raw binary
            hasher = hashlib.sha256()
            try:
                with open(target_bin, "rb") as f:  # audit-ignore-path  # fmt: skip
                    for chunk in iter(lambda: f.read(4096), b""):
                        hasher.update(chunk)
                if hasher.hexdigest() == checksum:
                    if not os.access(target_bin, os.X_OK):
                        os.chmod(target_bin, 0o750)
                    return target_bin
                else:
                    _logger.info("Checksum mismatch for %s, re-downloading...", cmd_name)
                    os.unlink(target_bin)  # audit-ignore-path  # fmt: skip
            except OSError as e:
                _logger.warning("Failed to check existing binary %s: %s", target_bin, e)

        try:
            head_req = urllib.request.Request(
                url, headers={"User-Agent": "HAMS-BinaryDownloader/1.0 (+https://yourdomain.com/bot-info)"}, method="HEAD"
            )
            try:
                with urllib.request.urlopen(head_req, timeout=15):
                    pass
            except urllib.error.URLError as e:
                _logger.warning("HEAD request failed for %s: %s", url, e)

            get_req = urllib.request.Request(
                url, headers={"User-Agent": "HAMS-BinaryDownloader/1.0 (+https://yourdomain.com/bot-info)"}
            )
            tmp_path = None
            try:
                with urllib.request.urlopen(get_req, timeout=15) as response:
                    etag = response.getheader("ETag")
                    if etag:
                        _logger.info("Download successful, ETag: %s", etag)

                    with tempfile.NamedTemporaryFile(dir=bin_dir, delete=False) as tmp:
                        tmp_path = tmp.name
                        for chunk in iter(lambda: response.read(8192), b""):
                            tmp.write(chunk)

                hasher = hashlib.sha256()
                with open(tmp_path, "rb") as f:  # audit-ignore-path  # fmt: skip
                    for chunk in iter(lambda: f.read(4096), b""):
                        hasher.update(chunk)

                if hasher.hexdigest() != checksum:
                    _logger.error(
                        "Checksum mismatch for %s. Expected %s, got %s",
                        cmd_name, checksum, hasher.hexdigest(),
                    )
                    raise UserError(
                        _("Security Alert: Checksum mismatch for downloaded %s binary.") % cmd_name
                    )

                if archive_type == "tar.gz":
                    with tarfile.open(tmp_path, "r:gz") as tar:  # audit-ignore-path  # fmt: skip
                        found = False
                        extract_target = extract_member or cmd_name
                        for member in tar:
                            if member.name.endswith(f"/{extract_target}") or member.name == extract_target:
                                if member.islnk() or member.issym():
                                    raise UserError(_("Security Alert: Links are not allowed in the archive."))
                                
                                member_filename = os.path.basename(member.name)
                                if not member_filename:
                                    continue

                                source = tar.extractfile(member)
                                if source:
                                    with source:
                                        with open(target_bin, "wb") as target:  # audit-ignore-path  # fmt: skip
                                            shutil.copyfileobj(source, target)
                                    found = True
                                    break
                        if not found:
                            raise UserError(_("Member %s not found in archive.") % extract_target)

                elif archive_type == "zip":
                    with zipfile.ZipFile(tmp_path, "r") as zip_ref:  # audit-ignore-path  # fmt: skip
                        extract_target = extract_member or cmd_name
                        found = False
                        for zinfo in zip_ref.infolist():
                            name = zinfo.filename
                            if name.endswith(f"/{extract_target}") or name == extract_target:
                                if stat.S_ISLNK(zinfo.external_attr >> 16):
                                    raise UserError(_("Security Alert: Links are not allowed in the archive."))

                                member_filename = os.path.basename(zinfo.filename)
                                if not member_filename:
                                    continue

                                with zip_ref.open(zinfo) as source:  # audit-ignore-path  # fmt: skip

                                    with open(target_bin, "wb") as target:  # audit-ignore-path  # fmt: skip
                                        shutil.copyfileobj(source, target)
                                found = True
                                break
                        if not found:
                            raise UserError(_("Member %s not found in zip archive.") % extract_target)
                else:
                    shutil.copy2(tmp_path, target_bin)

                os.chmod(target_bin, 0o750)
                return target_bin
            finally:
                if tmp_path and os.path.exists(tmp_path):
                    try:
                        os.unlink(tmp_path)  # audit-ignore-path  # fmt: skip
                    except OSError as e:
                        _logger.warning("Failed to remove temporary file %s: %s", tmp_path, e)
        except (UserError, ValidationError):
            raise
        except (urllib.error.URLError, OSError, tarfile.TarError, zipfile.BadZipFile) as e:
            _logger.exception("Failed to auto-install %s", cmd_name)
            raise UserError(_("Failed to auto-install %s: %s") % (cmd_name, str(e)))

    @api.model
    def _get_target_filename(self, cmd_name, checksum):
        """Generates a stable, unique filename based on the binary name and its checksum."""
        identifier = hashlib.sha256(f"{cmd_name}_{checksum}".encode()).hexdigest()[:16]
        return f"{cmd_name}_{identifier}"

    @api.model
    def _unlink_binary_file(self, cmd_name, checksum):
        """Safely removes a binary from the hams_bin directory."""
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        if ".." in cmd_name.split(os.path.sep):
            raise UserError(_("Security Alert: Path traversal attempt detected."))
        filename = self._get_target_filename(cmd_name, checksum)
        target_bin = os.path.realpath(os.path.join(bin_dir, filename))
        if not target_bin.startswith(os.path.realpath(bin_dir)):
            raise UserError(_("Security Alert: Path traversal attempt detected."))
        
        if os.path.exists(target_bin):
            try:
                os.unlink(target_bin)  # audit-ignore-path  # fmt: skip
            except OSError as e:
                _logger.warning("Failed to remove binary %s: %s", target_bin, e)
