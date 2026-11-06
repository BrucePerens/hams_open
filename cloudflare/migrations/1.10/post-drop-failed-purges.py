# Copyright © HAMS project. AGPL-3.0-or-later.
"""cloudflare 1.10: a purge for a website without a Cloudflare API token is dropped and paged, not parked as failed.

Before 1.10 such purges were set to state `failed` and never looked at again (7,014 rows by 2026-10-29, all
"missing API token or zone ID"). They are stale and cannot be sent, and a failed row now means a real Cloudflare
API error and raises a pager incident, so the old pile is deleted here. After a token is saved, run
Cloudflare > Manual Cache Purge with "Everything" for the website.

Raw SQL on the cursor, idempotent: with no failed rows it changes nothing."""


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "SELECT 1 FROM information_schema.columns "
        "WHERE table_name = 'cloudflare_purge_queue' AND column_name = 'state'"
    )
    if not cr.fetchone():
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "DELETE FROM cloudflare_purge_queue WHERE state = 'failed'"
    )
