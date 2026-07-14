# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
{
    "name": "Caching PWA",
    "version": "1.0",
    "author": "Bruce Perens K6BP",
    "category": "Website",
    "summary": "Global Service Worker for aggressive frontend asset caching",
    "description": "Intercepts network requests to cache Odoo JS/CSS bundles and static files on the client edge. Zero-config integration for other modules.",
    "depends": ["base", "website", "zero_sudo", "distributed_redis_cache"],
    "data": [
        "data/security_data.xml",
        "security/ir.model.access.csv",
        "views/res_config_settings_views.xml",
    ],
    "assets": {
        "web.assets_frontend": [
            "caching/static/src/js/register.js",
            "caching/static/src/js/toast.js",
        ],
        "web.assets_tests": [
            "caching/static/tests/tours/caching_tour.js",
        ],
    },
    "installable": True,
    "application": False,
    "license": "AGPL-3",
}
