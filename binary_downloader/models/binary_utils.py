# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

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
from urllib.parse import urlparse
from odoo import models, api, tools, fields, _
from odoo.exceptions import UserError, ValidationError
from odoo.addons.zero_sudo.daemon.ssrf_safe_fetch import (
    SSRFValidationError,
    is_ssrf_safe_public_ip as _shared_is_ssrf_safe_public_ip,
    resolve_ssrf_safe_addresses as _resolve_ssrf_safe_addresses,
    urlopen_ssrf_safe as _urlopen_ssrf_safe,
)

_logger = logging.getLogger(__name__)


# Bug-hunt finding, 2026-09-09 (binary_utils_download_and_extract, bug class
# 26-shaped): this module's own https-only/URL-scheme checks are real
# protection against ONE thing (a non-HTTP(S) scheme), but nothing here
# validated *where* an https:// URL actually points, or where it could be
# redirected to. An https:// manifest/version URL an admin or the
# binary_downloader manager group controls could 302-redirect to a
# loopback/link-local/private-use address (the classic
# http://169.254.169.254/... cloud-metadata target, or any other
# internal-only service reachable from the Odoo host) and urllib's default
# opener follows that redirect with no further check at all -- an SSRF
# primitive that lets whoever controls a manifest's URL make this server
# issue real network requests to internal targets, independent of whether
# the fetched bytes ever pass the checksum check that gates actual
# extraction/installation.
#
# Bug-hunt fix, 2026-09-14 (binary_utils_is_ssrf_safe_public_ip /
# binary_utils_assert_host_is_ssrf_safe): the classification predicate and
# the resolve+pin mechanism both now live in
# `zero_sudo.daemon.ssrf_safe_fetch` (imported above), shared with
# `pager_duty/daemon/pager_synthetic_spooler.py`'s own independently-
# duplicated copy of this exact check -- see that module's own docstring for
# why `zero_sudo` (a module both `binary_downloader` and `pager_duty` already
# depend on) is the shared home, and why this used to be a real
# DNS-rebinding TOCTOU (this predicate's own hostname validation and the
# actual `urlopen()` connection used to be two separate, independently-timed
# DNS lookups; the real fix pins the connection to the exact address this
# predicate already validated, rather than trusting a second lookup). Kept
# as a real, thin, anchored wrapper (not a bare re-export) so this anchor
# stays attached to a real function span for `check_claims_freshness.py`'s
# own AST-based hashing, and every existing caller/test of
# `_is_ssrf_safe_public_ip` keeps working unchanged.
# [@ANCHOR: binary_utils_is_ssrf_safe_public_ip]
def _is_ssrf_safe_public_ip(ip_obj):
    return _shared_is_ssrf_safe_public_ip(ip_obj)


# [@ANCHOR: binary_utils_safe_response_geturl]
# Verified by [@ANCHOR: test_safe_response_geturl_returns_none_for_an_object_without_geturl]
def _safe_response_geturl(response):
    """Returns response.geturl() if the object supports it, else None. Not every
    urlopen()-like response guarantees .geturl() -- this module's own tests mock
    urlopen() with plain io.BytesIO-based response doubles carrying no `geturl`
    attribute at all -- so this is standard, correct feature-detection on a
    stdlib-shaped object, not this codebase's own schema uncertainty. Callers
    should never need their own hasattr()/getattr()/except AttributeError for
    this; this is the one, tested, centralized place that pattern is allowed to
    live for this module (same standing fix as ingest/daemon_utils.py's
    try_enable_line_buffering(), per docs/proposals/LINTER_POLICY_REVISIT.md --
    check_burn_list.py's 3-arg-getattr() rule exempts this exact function by
    file path and function name, not by a copy-pasteable comment tag).
    """
    geturl = getattr(response, "geturl", None)
    if geturl is None:
        return None
    return geturl()


class BinaryDownloaderMixin(models.AbstractModel):
    _name = "binary_downloader.mixin"
    _description = "Binary Downloader Mixin"

    name = fields.Char(string="Name")

    @api.model
    # [@ANCHOR: binary_utils_download_and_extract]
    def _download_and_extract(self, cmd_name, url, checksum, archive_type, extract_member=None):
        """
        Downloads a binary, verifies its checksum, extracts it (if archive),
        and places it in the shared hams_bin directory.
        """
        if platform.system() != "Linux" or platform.machine() not in ("x86_64", "aarch64", "armv7l"):
            raise UserError(
                _("Auto-install of %s is only supported on Linux x86_64/aarch64/armv7l. Please install manually.") % cmd_name
            )
        
        # Bug-hunt fix, 2026-09-09: was "http:// and https://" here, weaker
        # than the https-only constraint binary.manifest/binary.version
        # already enforce (_check_url_scheme) on every real caller of this
        # mixin method. Harmless in practice today only because both real
        # callers pre-validate at the ORM layer before ever reaching this
        # code -- but a shared mixin method shouldn't offer a future caller
        # a weaker guarantee than its own module's stated security policy.
        if not url.startswith("https://"):
            raise UserError(_("Only https:// URLs are allowed."))

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

        # Bug-hunt fix, 2026-09-09: only reached once every cache-hit fast
        # path above has already returned -- an already-installed binary
        # (the overwhelmingly common case once a manifest has been used
        # once) never needs its URL resolved or reached at all, so this
        # check belongs here, immediately before the first real network
        # call, not any earlier.
        parsed_url = urlparse(url)
        self._assert_host_is_ssrf_safe(parsed_url.hostname, cmd_name)

        try:
            head_req = urllib.request.Request(
                url, headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36"}, method="HEAD"
            )
            try:
                # Bug-hunt fix, 2026-09-14: _urlopen_ssrf_safe() (not plain
                # urllib.request.urlopen()) resolves + validates + pins the
                # real connection to the validated address on every hop
                # (this request and any redirect it follows) -- see
                # zero_sudo/daemon/ssrf_safe_fetch.py's own docstring for
                # the DNS-rebinding TOCTOU this closes that the manual
                # _assert_host_is_ssrf_safe() pre-check above, by itself,
                # never could (it validates a hostname; urlopen() used to
                # re-resolve the SAME hostname separately, moments later,
                # to actually connect).
                with _urlopen_ssrf_safe(head_req, cmd_name, https_only=True, timeout=15):
                    pass
            except (urllib.error.URLError, SSRFValidationError) as e:
                _logger.warning("HEAD request failed for %s: %s", url, e)

            get_req = urllib.request.Request(
                url, headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36"}
            )
            tmp_path = None
            try:
                with _urlopen_ssrf_safe(get_req, cmd_name, https_only=True, timeout=15) as response:
                    # Bug-hunt fix, 2026-09-09: the pre-check above only
                    # validates the URL as *given*; urllib's default opener
                    # follows HTTP redirects on its own, with nothing
                    # re-validating where a redirect actually landed. An
                    # otherwise-safe https:// URL could 302 to a
                    # loopback/link-local/private-use target (SSRF) and the
                    # bytes fetched from THAT response are exactly what
                    # gets hashed and (if it somehow matched) extracted
                    # below -- re-check the resolved final URL's host
                    # before trusting anything read from this response.
                    # `response.geturl()` is only a plain string on a real
                    # urllib response; _safe_response_geturl() (above) guards
                    # against a test double that doesn't model it at all
                    # (confirmed live: several of this module's own tests
                    # mock urlopen() with plain io.BytesIO-based response
                    # doubles carrying no `geturl` attribute whatsoever) or
                    # returns a non-string, so neither can break this check.
                    final_url = _safe_response_geturl(response)
                    if isinstance(final_url, str) and final_url:
                        if not final_url.startswith("https://"):
                            raise UserError(
                                _(
                                    "Security Alert: redirect to a non-https:// "
                                    "URL rejected for %s."
                                )
                                % cmd_name
                            )
                        final_hostname = urlparse(final_url).hostname
                        if final_hostname:
                            self._assert_host_is_ssrf_safe(final_hostname, cmd_name)

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
                                        self._atomic_write_target(source, target_bin, bin_dir)  # audit-ignore-path  # fmt: skip
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
                                    self._atomic_write_target(source, target_bin, bin_dir)  # audit-ignore-path  # fmt: skip
                                found = True
                                break
                        if not found:
                            raise UserError(_("Member %s not found in zip archive.") % extract_target)
                else:
                    with open(tmp_path, "rb") as source:  # audit-ignore-path  # fmt: skip
                        self._atomic_write_target(source, target_bin, bin_dir)  # audit-ignore-path  # fmt: skip

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
        except SSRFValidationError as e:
            _logger.exception("SSRF safety check rejected a fetch for %s", cmd_name)
            raise UserError(_("Security Alert: %s") % str(e))
        except (urllib.error.URLError, OSError, tarfile.TarError, zipfile.BadZipFile) as e:
            _logger.exception("Failed to auto-install %s", cmd_name)
            raise UserError(_("Failed to auto-install %s: %s") % (cmd_name, str(e)))

    @api.model
    # [@ANCHOR: binary_utils_assert_host_is_ssrf_safe]
    def _assert_host_is_ssrf_safe(self, hostname, cmd_name):
        """
        Resolves `hostname` and raises UserError unless every address it
        resolves to is a safe public address (see _is_ssrf_safe_public_ip).
        Deliberately its own method, called separately for the URL as
        given and again for wherever a redirect actually lands
        (_download_and_extract), rather than inlined at either call site --
        so a test can mock exactly this one external dependency the same
        way this module's tests already mock urlopen()/shutil.which()/
        platform.*() instead of the test suite needing live DNS/network
        access to run.

        Bug-hunt fix, 2026-09-14: delegates to the shared, tested
        `zero_sudo.daemon.ssrf_safe_fetch.resolve_ssrf_safe_addresses()` (see
        that module's own docstring for the full DNS-rebinding TOCTOU
        writeup). This method, by itself, only ever validates a hostname --
        it cannot guarantee the connection `_download_and_extract()` makes a
        moment later actually goes to one of the addresses just checked here
        (that used to require a second, independently-timed
        `socket.getaddrinfo()` call inside `urlopen()` itself, which a
        DNS-rebinding attacker could answer differently). The real fix is
        `_download_and_extract()` calling `_urlopen_ssrf_safe()` instead of
        `urllib.request.urlopen()` directly, which pins the actual connection
        to the exact address this method validates; this method's own
        contract (raise `UserError` for a falsy/unresolvable/unsafe hostname,
        do nothing otherwise) is unchanged, so every existing caller and test
        of it keeps working exactly as before.
        """
        try:
            _resolve_ssrf_safe_addresses(hostname, cmd_name)
        except SSRFValidationError as e:
            raise UserError(_("Security Alert: %s") % str(e))

    @api.model
    # [@ANCHOR: binary_utils_atomic_write_target]
    def _atomic_write_target(self, source_fileobj, target_bin, bin_dir):
        """
        Writes source_fileobj's bytes to target_bin without ever exposing a
        partially-written file at that final path.

        Bug-hunt fix, 2026-09-09 (binary_utils_download_and_extract): before
        this, tar.gz/zip member extraction and the raw-binary copy both
        wrote directly to `open(target_bin, "wb")` / `shutil.copy2(...,
        target_bin)` -- not atomic. A process interrupted mid-write (an
        OOM kill, a container restart, `kill -9` during a slow install) could
        leave a truncated/corrupt file sitting at exactly target_bin's own
        path. For the tar.gz/zip case specifically, _download_and_extract's
        own "already installed" fast path (`if os.path.exists(target_bin):
        if archive_type != "binary": ... return target_bin`) then trusts
        that corrupt file FOREVER on every later call -- there's no stored
        hash for an individual extracted archive member to re-verify
        against on a cache hit, only for the whole downloaded archive
        (already checked before extraction even starts). Writing to a
        private temp file in the same directory (so the final os.replace is
        an atomic rename on the same filesystem) and only replacing
        target_bin once the write completes without error closes this: a
        target_bin that exists at all is now guaranteed to be the complete,
        untouched output of one successful extraction that already passed
        the whole-archive checksum check, not a partial write left by an
        interrupted attempt.
        """
        fd, tmp_target = tempfile.mkstemp(dir=bin_dir)
        try:
            with os.fdopen(fd, "wb") as target:  # audit-ignore-path  # fmt: skip
                shutil.copyfileobj(source_fileobj, target)
            os.replace(tmp_target, target_bin)  # audit-ignore-path  # fmt: skip
        except BaseException:  # audit-ignore-catch-all
            # Cleanup-then-reraise: must catch every kind of interruption (including
            # KeyboardInterrupt/SystemExit) to avoid leaking tmp_target, and always
            # re-raises unconditionally, so nothing is silently swallowed.
            if os.path.exists(tmp_target):  # audit-ignore-path  # fmt: skip
                try:
                    os.unlink(tmp_target)  # audit-ignore-path  # fmt: skip
                except OSError as e:
                    _logger.warning(
                        "Failed to remove leftover temp file %s: %s", tmp_target, e
                    )
            raise

    @api.model
    # [@ANCHOR: binary_utils_get_target_filename]
    def _get_target_filename(self, cmd_name, checksum):
        """Generates a stable, unique filename based on the binary name and its checksum."""
        identifier = hashlib.sha256(f"{cmd_name}_{checksum}".encode()).hexdigest()[:16]
        return f"{cmd_name}_{identifier}"

    @api.model
    # [@ANCHOR: binary_utils_unlink_binary_file]
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
