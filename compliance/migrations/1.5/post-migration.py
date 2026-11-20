# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Reword the Privacy Policy line on /compliance: it no longer says the platform protects personal data (ADR-0112: no privacy promises).

[@ANCHOR: compliance:post_migration_privacy_description]

The compliance.document record sits in a noupdate block, so `-u compliance` never rewrites it on an installed database. This
changes the description only where it still has the old shipped text, so a site owner's own wording is left alone."""

OLD = "How we collect, use, and protect your personal data."
NEW = "What personal data we collect and how it is used."


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the parameters are this file's own constants
        "UPDATE compliance_document SET description = %s WHERE description = %s"
        " AND id IN (SELECT res_id FROM ir_model_data WHERE module = 'compliance'"
        " AND name = 'doc_privacy_policy' AND model = 'compliance.document')",
        (NEW, OLD),
    )
