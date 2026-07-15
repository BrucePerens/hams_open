# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
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
    "external_dependencies": {
        "python": ["redis", "asyncpg", "python-dotenv"],
    },
    "depends": ["base", "zero_sudo", "daemon_key_manager"],
    "data": [
        "security/distributed_redis_cache_security.xml",
        "security/ir.model.access.csv",
        "views/distributed_cache_views.xml",
        "views/res_config_settings_views.xml",
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
