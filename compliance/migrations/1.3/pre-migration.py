# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Let edits to compliance's repo-authored "Accessibility Statement" template reach an
already-installed database.

[@ANCHOR: compliance:clear_stale_accessibility_template_noupdate_flag]

Same defect and fix as migrations/1.1 and 1.2: `compliance_accessibility_statement_template`
(data/legal_pages_data.xml) sat in a `<data noupdate="1">` block, so `-u compliance` skipped it and
the "Display and Reading Choices" section (hams_open #1304) did not reach hams_prod. Found by the
2026-11-08 upgrade rehearsal on a restored copy of hams_prod: after `-u all` the view's arch_db still
lacked that section. The template now sits in an ordinary `<data>` block; this pre-migration clears
the `noupdate = true` flag its existing `ir_model_data` row still carries, before the same `-u`
reloads the data files. The page record stays in the noupdate block and keeps its flag. Only list
records declared outside a noupdate block here.
tests/test_noupdate_migration.py checks this."""

TEMPLATE_XMLIDS = ("compliance_accessibility_statement_template",)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the only parameter is this file's own constant tuple of xmlid names, not user input
        "UPDATE ir_model_data SET noupdate = false"
        " WHERE module = 'compliance' AND name IN %s AND noupdate",
        (TEMPLATE_XMLIDS,),
    )
