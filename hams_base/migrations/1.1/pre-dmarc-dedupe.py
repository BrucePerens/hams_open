# Copyright © HAMS project. AGPL-3.0-or-later.
"""hams_base 1.1: one DMARC report per (org_name, report_id): delete older duplicates, keep the first.

[@ANCHOR: hams_base:migration_dmarc_report_dedupe]
Raw SQL on the cursor, idempotent: with no duplicates it changes nothing."""


def migrate(cr, version):
    if not version:
        return
    cr.execute("SELECT to_regclass('hams_base_dmarc_report')")
    if not cr.fetchone()[0]:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        """DELETE FROM hams_base_dmarc_report a USING hams_base_dmarc_report b
            WHERE a.org_name = b.org_name AND a.report_id = b.report_id AND a.id > b.id"""
    )
