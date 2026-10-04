# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "Parking: many domains, one lightweight Odoo",
    "version": "1.0",
    "summary": "Serve parked, redirected, for-sale or gone pages for any number of domains from one database",
    "description": (
        "Answers a request that arrives through Cloudflare for a hostname in the parking.domain table "
        "(and only for such a hostname) with a parked page, a redirect, a for-sale page or 410 Gone: "
        "no website, no session cookie, no Odoo frontend assets, no backend route. Built on tenant_sites, "
        "which decides which hostnames belong to the main site, to a tenant website or to parking."
    ),
    "author": "HAMS",
    "category": "Website",
    "depends": ["tenant_sites", "mail"],
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
    "application": False,
    "license": "AGPL-3",
}
