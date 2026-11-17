# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Let edits to compliance's privacy, cookie, terms, backup-policy and IP-disclosure templates reach an
already-installed database.

[@ANCHOR: compliance:clear_stale_legal_templates_noupdate_flag]

Same defect and fix as migrations/1.3: these five templates sat in `<data noupdate="1">` blocks, so `-u compliance`
skipped them and hams_prod kept the old arch (no page title or meta description from hams_open #2203, the old
Right to Erasure wording from #698, no table-responsive wrapper from #675). Found by the 2026-11-17 noupdate drift
audit, comparing hams_prod to the repo. The templates now sit in ordinary `<data>` blocks; this pre-migration clears
the `noupdate = true` flag their ir_model_data rows still carry, before the same `-u` reloads the data files. The
page and document records stay in the noupdate blocks. Only list records declared outside a noupdate block here.
tests/test_noupdate_migration.py checks this."""

TEMPLATE_XMLIDS = (
    "compliance_privacy_policy_template",
    "compliance_cookie_policy_template",
    "compliance_terms_of_service_template",
    "backup_policy_template",
    "intellectual_property_template",
)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; the only parameter is this file's own constant tuple of xmlid names, not user input
        "UPDATE ir_model_data SET noupdate = false"
        " WHERE module = 'compliance' AND name IN %s AND noupdate",
        (TEMPLATE_XMLIDS,),
    )
