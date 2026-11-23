# SPDX-License-Identifier: AGPL-3.0-or-later
"""pager_duty 1.3: the monitor's Odoo key moves to its family key directory.

pager-os-monitor runs as its own account, hamsd_pager_monitor (hams_shared MANIFEST), so register_pager_monitor_key updates the
registry row by name to /opt/hams/etc/keys/pager_monitor/pager_duty.env with that group and rotates the key into the new file. On a
host without the account the target stays the flat file (pager_monitor_key_target) and nothing changes. Idempotent."""

from odoo import api

from odoo.addons.pager_duty.hooks import register_pager_monitor_key


def migrate(cr, version):
    if not version:
        return
    # The pager service account's own environment, its id resolved from the external id with a plain query (no request user here).
    cr.execute(
        "SELECT res_id FROM ir_model_data WHERE module = 'pager_duty' AND name = 'user_pager_service_internal' AND model = 'res.users'"
    )
    row = cr.fetchone()
    if not row:
        return
    env = api.Environment(cr, row[0], {})
    if "daemon.key.registry" in env:
        register_pager_monitor_key(env)
