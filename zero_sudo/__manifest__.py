# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

{
    "name": "Zero-Sudo Security Core",
    "summary": "Foundational security utilities, service account patterns, and web isolation.",
    "description": """Zero-Sudo Security Core foundational module.

""",
    "author": "Bruce Perens K6BP",
    "category": "Security",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": ["base", "web", "mail", "distributed_redis_cache", "knowledge"],
    "assets": {
        "web.assets_backend": [
            "zero_sudo/static/src/components/security_dashboard/**/*",
        ],
        "web.assets_tests": [
            "zero_sudo/static/src/js/tour_utils.js",
            "zero_sudo/static/src/js/tour_failure_dump.js",
            "zero_sudo/static/src/tours/zero_sudo_tour.js",
        ],
    },
    "data": [
        "data/security_data.xml",
        "data/ir_cron.xml",
        "data/postgres_procedures.xml",
        "security/ir.model.access.csv",
        "security/ir_rule.xml",
        "views/res_users_views.xml",
        "views/security_log_views.xml",
        "data/noisy_table_data.xml",
        "views/noisy_table_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Zero-Sudo Security Core",
            "path": "data/documentation.html",
            "icon": "🛡️",
            "category": "workspace",
        },
        {
            "name": "Real Transaction Testing Facility Guide",
            "path": "data/testing_documentation.html",
            "icon": "🧪",
            "category": "workspace",
        },
        {
            "name": "Developer Integration Journey",
            "path": "docs/journeys/developer_integration.md",
            "icon": "🚀",
            "category": "workspace",
        },
        {
            "name": "Multi-Website Security Story",
            "path": "docs/stories/multi_website.md",
            "icon": "🌐",
            "category": "workspace",
        },
        {
            "name": "High-Performance Atomic KV Storage",
            "path": "docs/stories/set_kv_procedure.md",
            "icon": "⚡",
            "category": "workspace",
        },
    ],
    "installable": True,
    "auto_install": False,
}
