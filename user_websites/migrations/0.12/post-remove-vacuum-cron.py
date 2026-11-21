# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Remove the user_websites GDPR export token vacuum cron.

Policy (Bruce, 2026-11-20): stored information is never purged.

user_websites 0.11 shipped a daily cron that deleted ham.gdpr.export.token rows older than a day.
user_websites 0.12 keeps them forever; a token is still refused after its lifetime.

The cron record sat in a noupdate block (or has already been loaded), so removing it from the data
file alone would leave it running on installed databases. This post-migration unlinks it by xmlid.
Plain SQL, matching this platform's zero-sudo rule for migration scripts, idempotent: once the
xmlid is gone nothing matches. Rows the job would have deleted are not touched, and nothing here
deletes any user data.
"""

MODULE = "user_websites"

CRON_XMLIDS = (
    "ir_cron_vacuum_gdpr_export_tokens",
)

# Odoo also records an xmlid "<cron xmlid>_ir_actions_server" for the cron's inherited server action.
SERVER_ACTION_XMLIDS = tuple(name + "_ir_actions_server" for name in CRON_XMLIDS)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the only parameters are this file's own constants
        "SELECT res_id FROM ir_model_data"
        " WHERE module = %s AND name IN %s AND model = 'ir.cron'",
        (MODULE, CRON_XMLIDS),
    )
    cron_ids = [row[0] for row in cr.fetchall()]
    for cron_id in cron_ids:
        cr.execute(  # audit-ignore-sql: bound parameter, no user input
            "SELECT ir_actions_server_id FROM ir_cron WHERE id = %s", (cron_id,)
        )
        row = cr.fetchone()
        action_id = row[0] if row else None
        cr.execute("DELETE FROM ir_cron WHERE id = %s", (cron_id,))  # audit-ignore-sql: bound parameter
        if action_id:
            cr.execute("DELETE FROM ir_act_server WHERE id = %s", (action_id,))  # audit-ignore-sql: bound parameter
            cr.execute(  # audit-ignore-sql: bound parameter, constant type
                "DELETE FROM ir_actions WHERE id = %s AND type = 'ir.actions.server'", (action_id,)
            )
    cr.execute(  # audit-ignore-sql: static SQL; the only parameters are this file's own constants
        "DELETE FROM ir_model_data WHERE module = %s AND name IN %s",
        (MODULE, CRON_XMLIDS + SERVER_ACTION_XMLIDS),
    )
