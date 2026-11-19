# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""List every existing member in the community directory (opt-out model).
[@ANCHOR: user_websites:migrate_directory_listed_by_default]
Verified by [@ANCHOR: test_migration_directory_listed_by_default]

Bruce ruled 2026-11-19 (privacy review B8) that members are listed by default, with an opt-out. The column was a
plain Boolean defaulting off, so the database cannot tell "never chose" from "chose off": every active member whose
erasure is not pending is set to listed, and members opt out again if they wish. Suspended members stay out of the
directory through the view's own filter. Idempotent, plain SQL, nothing on a fresh install."""


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no parameters
        "UPDATE res_users SET privacy_show_in_directory = true"
        " WHERE active IS TRUE AND gdpr_erasure_pending IS NOT TRUE"
        " AND privacy_show_in_directory IS NOT TRUE"
    )
