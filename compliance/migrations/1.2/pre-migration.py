# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Let edits to compliance's repo-authored "Our Certification Authority" page reach an
already-installed database.

[@ANCHOR: compliance:clear_stale_ca_template_noupdate_flag]

Same defect and fix as migrations/1.1/pre-migration.py: `certification_authority_template`
(data/certification_authority_data.xml) sat in a `<data noupdate="1">` block, so `-u compliance`
skipped it and the 2026-10-12 rewrite of the page (the capability root is retired) did not reach
hams_prod. The template now sits in an ordinary `<data>` block; this pre-migration clears the
`noupdate = true` flag its existing `ir_model_data` row still carries, before the same `-u`
reloads the data files. Only list records declared outside a noupdate block here.
tests/test_noupdate_migration.py checks this."""

TEMPLATE_XMLIDS = ("certification_authority_template",)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the only parameter is this file's own constant tuple of xmlid names, not user input
        "UPDATE ir_model_data SET noupdate = false"
        " WHERE module = 'compliance' AND name IN %s AND noupdate",
        (TEMPLATE_XMLIDS,),
    )
