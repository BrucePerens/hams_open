# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
{
    "name": "Edge Cache",
    "version": "1.0",
    "summary": "Mark public website pages and assets cacheable at Cloudflare's edge, with no cookie on them",
    "description": (
        "Adds Cloudflare-CDN-Cache-Control to /web/assets and to website pages Odoo's own page cache "
        "accepts, never on a response that carries a Set-Cookie, and drops the redundant first-visit "
        "session_id and default frontend_lang cookies so an anonymous page can be cached. A small script "
        "fetches a fresh CSRF token before a form on such a page posts. Depends on website only (no "
        "Redis, no zero_sudo), so a tenant instance can use it. The cloudflare module depends on this one."
    ),
    "author": "HAMS",
    "category": "Website",
    # Deliberately only `website`: a tenant instance has no Redis, so zero_sudo and everything that
    # depends on it cannot be installed there (same choice as the parking module). The tests use the
    # project's Hams test base classes, which only need zero_sudo importable.
    "depends": ["website"],
    "data": [],
    "assets": {
        "web.assets_frontend": [
            "edge_cache/static/src/js/edge_cache_csrf.js",
        ],
    },
    "installable": True,
    "application": False,
    "license": "AGPL-3",
}
