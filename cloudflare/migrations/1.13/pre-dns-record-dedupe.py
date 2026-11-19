# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""cloudflare 1.13: one active DNS row per (type, name, content), enforced by a unique index.

[@ANCHOR: cloudflare:migration_dns_record_dedupe]
Where two or more active rows describe the same record, the one linked to Cloudflare by id (else the oldest) stays
active and the others are archived, never deleted: archiving removes nothing at Cloudflare. Raw SQL on the cursor,
idempotent: with no duplicates it changes nothing. Runs before the module update builds the index."""


def migrate(cr, version):
    if not version:
        return
    cr.execute("SELECT to_regclass('cloudflare_dns_record')")
    if not cr.fetchone()[0]:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        """UPDATE cloudflare_dns_record a SET active = FALSE
            WHERE a.active
              AND EXISTS (SELECT 1 FROM cloudflare_dns_record b
                           WHERE b.active AND b.id <> a.id AND b.type = a.type AND b.name = a.name
                             AND b.content = a.content
                             AND (CASE WHEN b.cf_record_id IS NOT NULL THEN 0 ELSE 1 END, b.id)
                               < (CASE WHEN a.cf_record_id IS NOT NULL THEN 0 ELSE 1 END, a.id))"""
    )
