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
        "/mnt/backup",
        # Bug-hunt fix (2026-09-09, tier-1 pass): this is the ONLY base under
        # which daemon/main.py's own, independent allowlist
        # (BACKUP_WORKER_SCRIPTS_DIR, default
        # "/opt/hams/daemons/backup_worker/scripts") will actually execute a
        # restore_drill_script -- none of the bases above satisfy the
        # daemon's own `abs_script_path.startswith(allowed_base + "/")`
        # check. Without this entry, this Odoo-side constraint accepted
        # restore_drill_script values (e.g. under /tmp or /opt/hams/backup)
        # that the daemon would always refuse to run, and rejected the one
        # path the daemon would accept -- a config that looked valid in the
        # UI could never actually run a drill. Class 6 (cross-section
        # self-contradiction between two independent allowlists for the same
        # field's value).
        "/opt/hams/daemons/backup_worker/scripts",
    ]
    # Bug-hunt fix (2026-09-09, tier-1 pass): "/opt/hams/etc/keys" was
    # previously in this list. No backup.config in this codebase (or its
    # tests) ever targets that directory -- it's this daemon's OWN
    # credential-file directory (see hooks.py's
    # env_file_path="/opt/hams/etc/keys/backup_worker.env"), not a
    # legitimate backup repository, restore destination, or drill-script
    # location. Allowing it here meant a backup admin could type
    # "/opt/hams/etc/keys" as a kopia restore_target_path and have the
    # daemon overwrite backup_worker.env (and any other daemon's credential
    # file living in that same directory) with the contents of a kopia
    # snapshot -- a real credential/privilege-compromise path, not just a
    # scoping papercut. Removed; nothing in this module needs it.
    #
    # Bug-hunt fix (2026-09-11): "/tmp" was also previously in this list --
    # confirmed by grepping this entire module (code, tests, docs) that
    # nothing legitimately targets it; the only reference anywhere was this
    # allowlist entry itself. A world-writable directory (mode 1777) has no
    # business being an allowed KOPIA_RESTORE_TARGET_PATH: any local user
    # could pre-stage a symlink there, or a restore into a predictable
    # /tmp path could plant/overwrite a file some OTHER privileged process
    # later reads trusting its own location. Same bug class, same fix
    # shape as the "/opt/hams/etc/keys" removal above -- a shared
    # allowlist function accepting a base directory nothing actually needs,
    # purely because it was convenient at some point. Removed.

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
        "'",
        '"',
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
