# Copyright © HAMS project. AGPL-3.0-or-later.
"""tenant_sites 1.1 adds tenant.site.static_service (the service that answers a site's /static/, which the
tunnel guard then insists is routed). Sites that already have such rules were set up by hand or by
devbox_tools/tenants_in_odoo_setup.py: adopt them so the guard protects them from the first push, with no
manual step. A site is adopted only when EVERY one of its hostnames has a hostname-scoped ^/static/ rule and
all of those rules name one service (http_status rules are not a service). Raw SQL, idempotent, and a
site that already has a value is left alone."""


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        "SELECT 1 FROM information_schema.columns WHERE table_name = 'tenant_site' AND column_name = 'static_service'"
    )
    if not cr.fetchone():
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values
        """
        SELECT s.id, h.name, r.service_url
          FROM tenant_site s
          JOIN tenant_site_host h ON h.site_id = s.id
          LEFT JOIN cloudflare_tunnel_route r
                 ON r.hostname = h.name AND r.path = '^/static/' AND r.service_url NOT LIKE 'http_status:%%'
         WHERE s.active AND COALESCE(s.static_service, '') = ''
        """
    )
    by_site = {}
    for site_id, _host, service in cr.fetchall():
        by_site.setdefault(site_id, []).append(service)
    for site_id, services in by_site.items():
        distinct = set(services)
        if None in distinct or len(distinct) != 1:
            continue
        cr.execute(  # audit-ignore-sql: values are bound parameters
            "UPDATE tenant_site SET static_service = %s WHERE id = %s", (distinct.pop(), site_id)
        )
