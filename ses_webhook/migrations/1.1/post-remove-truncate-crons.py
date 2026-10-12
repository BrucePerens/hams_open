# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Remove the ses_webhook log and pending-submission truncation crons.

Bruce, 2026-10-11: "We retain data forever" / "We make no promises to delete anything."

ses_webhook 1.0 shipped two daily crons that deleted ses.webhook.log rows older than 30 days and
ses.webhook.pending_submission rows older than 7 days. ses_webhook 1.1 keeps both forever.

The cron record sat in a noupdate block (or has already been loaded), so removing it from the data
file alone would leave it running on installed databases. This post-migration unlinks it by xmlid.
Plain SQL, matching this platform's zero-sudo rule for migration scripts, idempotent: once the
xmlid is gone nothing matches. Rows the job would have deleted are not touched, and nothing here
deletes any user data.
"""

MODULE = "ses_webhook"

CRON_XMLIDS = (
    "ir_cron_truncate_ses_webhook_logs",
    "ir_cron_truncate_ses_webhook_pending_submissions",
)


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
            cr.execute("DELETE FROM ir_actions WHERE id = %s", (action_id,))  # audit-ignore-sql: bound parameter
    cr.execute(  # audit-ignore-sql: static SQL; the only parameters are this file's own constants
        "DELETE FROM ir_model_data WHERE module = %s AND name IN %s",
        (MODULE, CRON_XMLIDS),
    )
