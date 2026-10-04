# Copyright © HAMS project. AGPL-3.0-or-later.
"""cloudflare 1.7 gives cloudflare.dns.record a push to Cloudflare.

Until 1.7 the rows were notes (data only) of records that were created at Cloudflare by other means
(stun.hams.com, the tenant CNAMEs). `manage` defaults to True for new rows, which would have made every
row that already exists writable by a push. This post-migration turns it off for the rows that
exist at upgrade time, so a push links them and reports differences but writes nothing until an
administrator deliberately ticks "Odoo manages it". It also clears `proxied` on NS and TXT rows,
which can never be proxied (a constraint now refuses it).

Raw SQL on the cursor (no ORM), idempotent, and it only runs when upgrading from a version below 1.7."""


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "SELECT 1 FROM information_schema.columns "
        "WHERE table_name = 'cloudflare_dns_record' AND column_name = 'manage'"
    )
    if not cr.fetchone():
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "UPDATE cloudflare_dns_record SET manage = false WHERE manage IS DISTINCT FROM false"
    )
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "UPDATE cloudflare_dns_record SET proxied = false WHERE type IN ('NS', 'TXT') AND proxied IS DISTINCT FROM false"
    )
