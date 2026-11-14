# Copyright © HAMS project. AGPL-3.0-or-later.
"""cloudflare 1.12: one tunnel record per (website, Cloudflare tunnel id): delete older duplicates, keep the first.

[@ANCHOR: cloudflare:migration_tunnel_pair_dedupe]
Raw SQL on the cursor, idempotent: with no duplicates it changes nothing."""


def migrate(cr, version):
    if not version:
        return
    cr.execute("SELECT to_regclass('cloudflare_tunnel')")
    if not cr.fetchone()[0]:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        """DELETE FROM cloudflare_tunnel a USING cloudflare_tunnel b
            WHERE a.website_id = b.website_id AND a.cf_tunnel_id = b.cf_tunnel_id AND a.id > b.id"""
    )
