# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""hams_helpdesk 1.4: the exception reporter's key moves to its family key directory.

hams-exception-report@ runs as its own account, hamsd_exception_report (hams_shared MANIFEST), so the key is written to
/opt/hams/etc/keys/exception_report/exception_reporter.env with that group. register_daemon updates the existing registry row by name
(path and group) and rotates the key into the new file. On a host where the account does not exist yet the target stays the flat file
(exception_reporter_key_target), so the upgrade changes nothing there; run it again after provisioning. Idempotent."""

from odoo.addons.hams_helpdesk.hooks import register_exception_reporter_key
from odoo.addons.hams_helpdesk.models.exception_fingerprint import _service_env


def migrate(cr, version):
    if not version:
        return
    register_exception_reporter_key(_service_env(cr))
