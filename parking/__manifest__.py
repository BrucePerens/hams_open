# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "Parking: many domains, one lightweight Odoo",
    "version": "1.0",
    "summary": "Serve parked, redirected, for-sale or gone pages for any number of domains from one database",
    "description": (
        "For a dedicated, loopback-administered Odoo instance that sits behind a Cloudflare Tunnel "
        "catch-all rule. Every request whose Host is not an administration host is answered from the "
        "parking.domain table by the ir.http fallback: no website, no session cookie, no Odoo "
        "frontend assets, no backend route reachable from the internet."
    ),
    "author": "HAMS",
    "category": "Website",
    # Deliberately no zero_sudo: a tenant instance has no Redis, so zero_sudo and everything that
    # depends on it cannot be installed there. The privilege separation zero_sudo provides is done
    # here with one service account that holds exactly the ACLs the public handler needs. (The tests
    # use the project's Hams test base classes, which only need zero_sudo importable.)
    "depends": ["web", "mail"],
    "data": [
        "security/parking_security.xml",
        "security/ir.model.access.csv",
        "data/parking_data.xml",
        "views/parking_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Parking domains",
            "path": "data/documentation.html",
            "icon": "P",
            "category": "workspace",
        }
    ],
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": True,
    "license": "AGPL-3",
}
