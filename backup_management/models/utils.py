# -*- coding: utf-8 -*-
import os
import logging
from odoo.exceptions import UserError
from odoo import _

_logger = logging.getLogger(__name__)


def validate_backup_path(path):
    # [@ANCHOR: backup_path_validation]
    # Verified by [@ANCHOR: test_backup_security]
    if not path:
        return

    # Resolve symlinks to check the actual target path
    # We use normpath first then realpath
    try:
        abs_path = os.path.realpath(os.path.normpath(path))
    except OSError as e:
        _logger.warning("Failed to resolve realpath for %s: %s", path, e)
        abs_path = os.path.abspath(path)

    # Block sensitive system directories
    forbidden = [
        "/etc",
        "/root",
        "/boot",
        "/sys",
        "/proc",
        "/dev",
        "/bin",
        "/sbin",
        "/lib",
        "/usr/bin",
        "/usr/sbin",
        "/usr/lib",
        "/home",
        "/var/log",
        "/var/cache",
        "/var/spool",
        "/var/run",
        "/run",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/local/lib",
    ]

    if any(abs_path == f or abs_path.startswith(f + "/") for f in forbidden):
        raise UserError(
            _("Access to the path %s is prohibited for security reasons.") % path
        )

    # Ensure it's not trying to overwrite Odoo core or sensitive data
    if abs_path.startswith("/var/lib/odoo"):
        # Allow /var/lib/odoo/backups or similar if needed, but block core dirs
        # We explicitly allow /var/lib/odoo/backups and /var/lib/odoo/daemon_keys (read-only usually)
        # but block critical ones.
        blocked_odoo = [
            "/var/lib/odoo/sessions",
            "/var/lib/odoo/addons",
            "/var/lib/odoo/filestore",
        ]
        if any(abs_path == f or abs_path.startswith(f + "/") for f in blocked_odoo):
            raise UserError(
                _("Access to internal Odoo data directory %s is prohibited.") % path
            )

    # Prevent command injection via flags if path is used in CLI
    if path.startswith("-"):
        raise UserError(_("Invalid path: path cannot start with a hyphen."))

    # Block shell metacharacters
    metacharacters = [
        ";",
        "&",
        "|",
        "`",
        "$",
        "(",
        ")",
        "<",
        ">",
        "*",
        "?",
        "[",
        "]",
        "{",
        "}",
        "\n",
    ]
    if any(char in path for char in metacharacters):
        raise UserError(_("Invalid path: path contains illegal characters."))

    # Block recursive directory traversal and other suspicious patterns
    if ".." in path.split(os.path.sep):
        raise UserError(_("Invalid path: directory traversal is not allowed."))
