# Copyright © HAMS project. AGPL-3.0-or-later.
"""Bruce, 2026-10-03: "Don't expose ssh."

cloudflare 1.4 added cloudflare.tunnel.ssh_route_enabled defaulting to True, which made every
Push publish ssh.<domain> -> ssh://localhost:22 to Cloudflare. 1.5 defaults the field to False,
and this post-migration switches it off on every tunnel that already exists, so a Push can never
publish SSH unless an administrator deliberately turns it back on afterwards.

Raw SQL on the cursor (no ORM), idempotent: re-running changes nothing once every row is False.
Rows are only touched while the column holds True or NULL, so a later deliberate opt-in is not
undone by running the upgrade again at 1.5 or later (this script only runs when upgrading from a
version below 1.5)."""


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "SELECT 1 FROM information_schema.columns "
        "WHERE table_name = 'cloudflare_tunnel' AND column_name = 'ssh_route_enabled'"
    )
    if not cr.fetchone():
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "UPDATE cloudflare_tunnel SET ssh_route_enabled = false "
        "WHERE ssh_route_enabled IS DISTINCT FROM false"
    )
