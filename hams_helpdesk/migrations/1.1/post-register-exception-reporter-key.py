# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""hams_helpdesk 1.1: issue the exception reporter service account's daemon key.

1.0 had no external reporter; 1.1 adds ``report_daemon_failure`` and the OnFailure reporter unit that
needs a key for the account that is the only one allowed to call it. ``post_init_hook`` only runs on a
fresh install, so an installed database registers the key here. Idempotent."""

from odoo.addons.hams_helpdesk.hooks import register_exception_reporter_key
from odoo.addons.hams_helpdesk.models.exception_fingerprint import _service_env


def migrate(cr, version):
    if not version:
        return
    # The reporter service account's own environment (resolved by xmlid with a plain query), not a
    # superuser one.
    register_exception_reporter_key(_service_env(cr))
