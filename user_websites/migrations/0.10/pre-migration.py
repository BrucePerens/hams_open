# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Let edits to user_websites' three mail templates reach an already-installed database.
[@ANCHOR: user_websites:migrate_clear_mail_template_noupdate_flags]
Verified by [@ANCHOR: test_migration_clear_mail_template_noupdate_flags]

The Content Violation Report, Pending Violations Summary and Weekly Digest templates sat in a
`<data noupdate="1">` block, so `-u user_websites` skipped them and hams_prod kept the old sender
('noreply@yourcompany.com' or the reporter's typed address, hams_open #1323, #2442), the old recipient
fallback and the old report body. Found by the 2026-11-17 noupdate drift audit, comparing hams_prod to the
repo. The templates now sit in an ordinary `<data>` block; this pre-migration clears the `noupdate = true`
flag their ir_model_data rows still carry, before the same `-u` reloads the data file. Idempotent, plain SQL,
nothing on a fresh install."""

TEMPLATE_XMLIDS = (
    "email_template_content_violation",
    "email_template_pending_violations_summary",
    "email_template_weekly_digest",
)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the only parameter is this file's own constant tuple of xmlid names, not user input
        "UPDATE ir_model_data SET noupdate = false"
        " WHERE module = 'user_websites' AND model = 'mail.template' AND name IN %s AND noupdate",
        (TEMPLATE_XMLIDS,),
    )
