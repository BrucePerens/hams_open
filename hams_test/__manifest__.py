# -*- coding: utf-8 -*-
{
    "name": "Hams Test Infrastructure",
    "version": "1.0",
    "author": "Bruce Perens K6BP",
    "category": "Hidden",
    "summary": "Unified testing infrastructure (Real Transaction, Tours, Integration Daemons).",
    "depends": ["base", "web", "web_tour", "zero_sudo"],
    "data": [
        "security/security.xml",
        "security/ir.model.access.csv",
        "data/noisy_table_data.xml",
        "views/noisy_table_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Testing Infrastructure Guide",
            "path": "data/documentation.html",
            "icon": "🧪",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_tests": [
            "hams_test/static/src/js/tour_utils.js",
            "hams_test/static/src/js/tour_failure_dump.js",
            "hams_test/static/src/js/tours/**/*",
        ],
    },
    "license": "AGPL-3",
    "installable": True,
    "auto_install": False,
}
