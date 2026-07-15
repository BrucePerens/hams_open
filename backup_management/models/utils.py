# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import os
import logging
from odoo.exceptions import UserError
from odoo import _

_logger = logging.getLogger(__name__)


def validate_backup_path(path):
    # [@ANCHOR: backup_management:COMM_backup_path_validation]

    # # Verified by [@ANCHOR: backup_management:COMM_test_backup_security]
    if not path:
        return

    # Resolve symlinks to check the actual target path
    # We use normpath first then realpath
    try:
        abs_path = os.path.realpath(os.path.normpath(path))
    except OSError as e:
        _logger.warning("Failed to resolve realpath for %s: %s", path, e)
        raise UserError(_("Access denied: Unable to securely resolve the path (possible symlink loop)."))

    allowed_bases = [
        "/var/lib/odoo/backups",
        "/var/lib/odoo/backup_repo",
        "/var/backups/global",
        "/opt/hams/backup",
        "/opt/hams/etc/keys",
        "/mnt/backup",
        "/tmp"
    ]

    if not any(abs_path == base or abs_path.startswith(base + "/") for base in allowed_bases):
        raise UserError(
            _("Access to the path %s is prohibited. Must be within allowed backup directories.") % path
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
        "\r",
        "\\",
    ]
    if any(char in path for char in metacharacters):
        raise UserError(_("Invalid path: path contains illegal characters."))

    # Block recursive directory traversal and other suspicious patterns
    if ".." in path.split(os.path.sep):
        raise UserError(_("Invalid path: directory traversal is not allowed."))

def publish_to_rabbitmq(env, msg):
    """
    Publishes a message to RabbitMQ backup_tasks queue using the global connection pool.
    """
    try:
        env["hams_rabbitmq.pool"].publish(
            "", "backup_tasks", msg
        )
    except Exception as e:  # audit-ignore-catch-all: # Tested by [@ANCHOR: backup_management:COMM_test_rmq_publish_failure]  # fmt: skip
        _logger.exception("Failed to publish backup task to RMQ pool: %s", e)
