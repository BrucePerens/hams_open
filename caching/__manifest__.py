# Copyright © HAMS project. AGPL-3.0-or-later.
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
        "data/pwa_offline_template.xml",
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
            "caching/static/tests/tours/sw_behavior_tour.js",
        ],
        "web.assets_unit_tests": [
            # toast.js also has to be listed here, not just in
            # web.assets_frontend above: /web/tests's own
            # assets_unit_tests_setup bundle only ('include's)
            # web.assets_backend, never web.assets_frontend -- same gap
            # hams_com/ham_shack/__manifest__.py's own sdr_spectrum.js
            # comment documents for the identical reason.
            "caching/static/src/js/toast.js",
            "caching/static/tests/toast.test.js",
        ],
    },
    "installable": True,
    "application": False,
    "license": "AGPL-3",
    "post_init_hook": "_post_init_hook",
}
