# -*- coding: utf-8 -*-
{
    "name": "Distributed Redis Cache",
    "summary": "Fine-grained distributed caching for Odoo clusters.",
    "description": """
Distributed Redis Cache
=======================
* Fine-grained distributed caching and phase-coherence.
* Speed optimization by invalidating single databases.
* Replaces Odoo's internal cache.
* Includes a UI to manage the cache and check Redis status.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "category": "Technical",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": ["base", "zero_sudo"],
    "data": [
        "security/distributed_redis_cache_security.xml",
        "security/ir.model.access.csv",
        "views/distributed_cache_views.xml",
    ],
    "assets": {
        "web.assets_tests": [
            "distributed_redis_cache/static/src/js/distributed_cache_tour.js",
        ],
    },
    "knowledge_docs": [
        {
            "name": "Distributed Redis Cache",
            "path": "data/documentation.html",
            "icon": "⚡",
            "category": "workspace",
        }
    ],
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
    "auto_install": False,
}
