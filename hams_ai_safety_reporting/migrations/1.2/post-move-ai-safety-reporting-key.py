# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""hams_ai_safety_reporting 1.2: the safety reporting key moves to the simulated bots' family account.

Bruce, 2026-11-19: the simulated bots and the Official Observer share one low-privilege account, hamsd_simbots. The registry row
moves from the flat odoo-owned key file to the family key directory, /opt/hams/etc/keys/simbots, with ``os_group`` hamsd_simbots, and a
fresh key is written there (``register_ai_safety_reporting_key``; it keeps the row's name). The units switch to the new path when
provisioning applies them: docs/proposals/DAEMON_OS_ISOLATION_PLAN.md in hams_com. A host without the hamsd_simbots group is left as
it is (the function logs and returns False). A database from before 1.1 gets the registration from migrations/1.1 already, so it is
not done twice. The old flat file stays for an operator to remove. Idempotent in rows."""

from odoo import api

from odoo.addons.hams_ai_safety_reporting.hooks import (
    SERVICE_XMLID,
    register_ai_safety_reporting_key,
)


# [@ANCHOR: hams_ai_safety_reporting:migrate_move_key]
def migrate(cr, version):
    if not version or version < "1.1":
        return
    module, name = SERVICE_XMLID.split(".", 1)
    cr.execute(
        "SELECT res_id FROM ir_model_data WHERE module = %s AND name = %s AND model = 'res.users'",
        (module, name),
    )
    row = cr.fetchone()
    if not row:
        raise ValueError(f"{SERVICE_XMLID} is missing")
    register_ai_safety_reporting_key(api.Environment(cr, row[0], {}))
