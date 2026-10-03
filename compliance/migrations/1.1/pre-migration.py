# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Let edits to compliance's repo-authored "How Hams.com Protects Hams" page reach an
already-installed database.

[@ANCHOR: compliance:clear_stale_template_noupdate_flags]

`protects_hams_template` (data/protects_hams_data.xml) used to sit in a `<data noupdate="1">`
block. On every `-u` after the first install, Odoo 19's `XmlImport._tag_record()` skips a record in
such a block when its xmlid already exists, so later edits to the template never applied. hams_open
#457 (480943ad, 2026-09-23) added the "We are not exposed to a ransomware pay-to-recover scenario"
section. A read-only check of hams_prod on 2026-10-02 found that section in the deployed source
(/opt/hams/src/hams_open) but not in the view's arch_db. The view's write_date was still its
2026-09-21 install time, and there is no per-website copy of the view.

The template now sits in an ordinary `<data>` block. Moving it does not clear the `noupdate = true`
flag on its existing `ir_model_data` row: `_update_xmlids()` never rewrites that column, and
`_load_records()` skips the write while it is set. hams_prod and hams_dev both have the flag set.
This pre-migration clears it before the same `-u` reloads the data files, so that upgrade rewrites
the view to the current XML.

Plain SQL, matching this platform's zero-sudo rule for migration scripts. Rows already at false are
left alone. Only list records declared outside a noupdate block here: clearing the flag of a record
meant to stay noupdate (protects_hams_page, doc_protects_hams) would let `-u` overwrite it.
tests/test_noupdate_migration.py checks this."""

TEMPLATE_XMLIDS = ("protects_hams_template",)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the only parameter is this file's own constant tuple of xmlid names, not user input
        "UPDATE ir_model_data SET noupdate = false"
        " WHERE module = 'compliance' AND name IN %s AND noupdate",
        (TEMPLATE_XMLIDS,),
    )
