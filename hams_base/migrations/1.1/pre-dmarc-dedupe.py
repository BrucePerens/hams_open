# Copyright © HAMS project. AGPL-3.0-or-later.
"""hams_base 1.1: one DMARC report per (org_name, report_id): the first keeps its id, later duplicates are renamed.

[@ANCHOR: hams_base:migration_dmarc_report_dedupe]
Reports are records of what receivers saw, so none is deleted (logs are permanent): a later duplicate gets
report_id "<id> (duplicate <row id>)", which frees the (org_name, report_id) key for the unique constraint.
Raw SQL on the cursor, idempotent: with no duplicates it changes nothing."""


def migrate(cr, version):
    if not version:
        return
    cr.execute("SELECT to_regclass('hams_base_dmarc_report')")
    if not cr.fetchone()[0]:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        """UPDATE hams_base_dmarc_report a SET report_id = a.report_id || ' (duplicate ' || a.id || ')'
            WHERE EXISTS (SELECT 1 FROM hams_base_dmarc_report b
                           WHERE a.org_name = b.org_name AND a.report_id = b.report_id AND a.id > b.id)"""
    )
