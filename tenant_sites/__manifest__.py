# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "Tenant sites: other websites on this Odoo",
    "version": "1.1",
    "summary": "Serve other sites (perens.com, postopen.org, ...) as websites of one Odoo, isolated from the main site",
    "description": (
        "A tenant is a website bound to its own hostnames. A request that arrives through Cloudflare "
        "for a tenant's hostname gets that website's own public content, read-only; nothing of the "
        "main site, no backend, no login, no JSON or XML-RPC. A hostname that is neither the main "
        "site's, a tenant's nor a parked domain's gets an uncached 404."
    ),
    "author": "HAMS",
    "category": "Website",
    "depends": ["website", "website_blog", "edge_routing", "zero_sudo", "cloudflare"],
    "data": [
        "security/security_data.xml",
        "security/ir.model.access.csv",
        "views/tenant_site_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Tenant sites",
            "path": "data/documentation.html",
            "icon": "T",
            "category": "workspace",
        }
    ],
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
    "license": "AGPL-3",
}
