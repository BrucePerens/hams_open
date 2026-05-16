{
    "name": "Manual Library",
    "summary": "Hierarchical documentation and knowledge-base system",
    "description": """
A clean-room, open-source implementation of a hierarchical documentation
system for Odoo Community. Provides API interoperability for the
knowledge.article model. Includes frontend search, feedback, and dynamic TOC.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com",
    "category": "Website",
    "version": "1.1",
    "license": "AGPL-3",
    "depends": [
        "base",
        "web",
        "mail",
        "website",
        "zero_sudo",
        "hams_test",
    ],
    "external_dependencies": {
        "python": [],
    },
    "data": [
        "security/manual_library_security.xml",
        "security/ir.model.access.csv",
        "views/knowledge_article_views.xml",
        "views/knowledge_article_templates.xml",
    ],
    "knowledge_docs": [
        {"path": "README.md", "name": "Developer Guide", "icon": "🛠️"},
        {"path": "data/documentation.html", "name": "User Manual", "icon": "📖"},
    ],
    "assets": {
        "web.assets_frontend": [
            "manual_library/static/src/js/manual_toc.js",
        ],
        "web.assets_tests": [
            "manual_library/static/tests/tours/**/*",
        ],
    },
    "installable": True,
    "application": True,
}
