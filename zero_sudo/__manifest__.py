# -*- coding: utf-8 -*-
{
    "name": "Zero-Sudo Security Core",
    "summary": "Foundational security utilities, service account patterns, and web isolation.",
    "description": "Zero-Sudo Security Core foundational module.",
    "author": "Bruce Perens K6BP",
    "category": "Security",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": ["base", "mail"],
    "assets": {
      "web.assets_tests": [
        "zero_sudo/static/src/**/*"
      ],
    },
    "data": [
        "data/security_data.xml",
        "security/ir.model.access.csv",
        "views/res_users_views.xml"
    ],
    "knowledge_docs": [
        {
            "name": "Zero-Sudo Security Core",
            "path": "data/documentation.html",
            "icon": "🛡️",
            "category": "workspace"
        },
        {
            "name": "Developer Integration Journey",
            "path": "docs/journeys/developer_integration.md",
            "icon": "🚀",
            "category": "workspace"
        },
        {
            "name": "Multi-Website Security Story",
            "path": "docs/stories/multi_website.md",
            "icon": "🌐",
            "category": "workspace"
        }
    ],
    "installable": True,
    "auto_install": False,
}
