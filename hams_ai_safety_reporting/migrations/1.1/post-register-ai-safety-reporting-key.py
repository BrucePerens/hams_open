# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""hams_ai_safety_reporting 1.1: register the service account's daemon key.

1.0 never registered it: the key behind the Simulated Band bots' safety tickets was added to the
registry by hand on hams.com, so a fresh install had no key and the bots could not file a ticket.
``post_init_hook`` only runs on a fresh install, so an installed database registers the key here
(an existing row for the same key file is kept, see ``register_ai_safety_reporting_key``). Idempotent."""

from odoo import api

from odoo.addons.hams_ai_safety_reporting.hooks import (
    SERVICE_XMLID,
    register_ai_safety_reporting_key,
)


# [@ANCHOR: hams_ai_safety_reporting:migrate]
def migrate(cr, version):
    if not version:
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
