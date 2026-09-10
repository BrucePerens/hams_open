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
        # burn-ignore-hoot-runner-coverage: this module's hoot unit
        # tests below are registered here but have no tests/test_*.py
        # runner that actually executes them via browser_js() -- a real,
        # tracked gap (found 2026-09-09 while fixing a separate,
        # widespread window.fetch hoot-mocking bug), not an intentional
        # design choice. See check_hoot_runner_coverage.py.
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
