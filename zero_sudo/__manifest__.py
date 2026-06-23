# -*- coding: utf-8 -*-
{
    "name": "Zero-Sudo Security Core",
    "summary": "Foundational security utilities, service account patterns, and web isolation.",
    "description": """Zero-Sudo Security Core foundational module.

""",
    "author": "Bruce Perens K6BP",
    "category": "Security",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": ["base", "web", "mail"],
    "assets": {
      "web.assets_tests": [
        "zero_sudo/static/src/js/tour_utils.js",
        "zero_sudo/static/src/js/tour_failure_dump.js",
        "zero_sudo/static/src/tours/zero_sudo_tour.js"
      ],
    },
    "data": [
        "data/security_data.xml",
        "data/postgres_procedures.xml",
        "security/ir.model.access.csv",
        "views/res_users_views.xml",
        "views/security_log_views.xml",
        "data/noisy_table_data.xml",
        "views/noisy_table_views.xml"
    ],
    "knowledge_docs": [
        {
            "name": "Zero-Sudo Security Core",
            "path": "data/documentation.html",
            "icon": "🛡️",
            "category": "workspace"
        },
        {
            "name": "Real Transaction Testing Facility Guide",
            "path": "data/testing_documentation.html",
            "icon": "🧪",
            "category": "workspace"
        },
        {
            "name": "Developer Integration Journey",
            "path": "hams_shared/docs/journeys/developer_integration.md",
            "icon": "🚀",
            "category": "workspace"
        },
        {
            "name": "Multi-Website Security Story",
            "path": "hams_shared/docs/stories/multi_website.md",
            "icon": "🌐",
            "category": "workspace"
        },
        {
            "name": "High-Performance Atomic KV Storage",
            "path": "hams_shared/docs/stories/set_kv_procedure.md",
            "icon": "⚡",
            "category": "workspace"
        }
    ],
    "installable": True,
    "auto_install": False,
}
