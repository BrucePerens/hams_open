# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Drop the retired LoTW claim from the already-installed "How Hams.com Protects Hams" registry entry.

[@ANCHOR: compliance:protects_hams_description_without_lotw]

hams.com no longer uses ARRL Logbook of the World (2026-10-23). The page template reloads on
`-u compliance` (it sits outside the noupdate block), but its `compliance.document` record
(`doc_protects_hams`) is noupdate, so its description would keep promising "LoTW credential
custody" on /compliance. Rewrite it only while it still holds the exact old repo-authored text, so
an administrator's own edit is never overwritten. Nothing is changed on a fresh install.
tests/test_noupdate_migration.py checks this."""

OLD_DESCRIPTION = (
    "A central point explaining every real, technical way we protect you: LoTW credential custody, "
    "why we can never control your transmitter, disclosed source, signing, and reproducible builds."
)
NEW_DESCRIPTION = (
    "A central point explaining every real, technical way we protect you: "
    "why we can never control your transmitter, disclosed source, signing, and reproducible builds."
)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL; parameters are this file's own constants, not user input
        "UPDATE compliance_document SET description = %s"
        " WHERE description = %s AND id IN ("
        "  SELECT res_id FROM ir_model_data"
        "  WHERE module = 'compliance' AND name = 'doc_protects_hams'"
        "  AND model = 'compliance.document')",
        (NEW_DESCRIPTION, OLD_DESCRIPTION),
    )
