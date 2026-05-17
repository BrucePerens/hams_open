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
        {"path": "README.md", "name": "Manual Library: Developer Guide", "icon": "🛠️", "category": "workspace"},
        {"path": "data/documentation.html", "name": "Manual Library: User Guide", "icon": "📖", "category": "workspace"},
        {"path": "docs/journeys/admin_managing_articles.md", "name": "Journey: Administrator Managing Articles", "icon": "🚀", "category": "workspace"},
        {"path": "docs/journeys/developer_doc_integration.md", "name": "Journey: Developer Integrating Documentation", "icon": "💻", "category": "workspace"},
        {"path": "docs/journeys/user_browsing_journey.md", "name": "Journey: User Browsing the Manual", "icon": "👤", "category": "workspace"},
        {"path": "docs/stories/article_view.md", "name": "Story: Viewing Manual Articles", "icon": "📄", "category": "workspace"},
        {"path": "docs/stories/backend_views.md", "name": "Story: Backend Management Views", "icon": "⚙️", "category": "workspace"},
        {"path": "docs/stories/doc_installation.md", "name": "Story: Automated Documentation Installation", "icon": "📦", "category": "workspace"},
        {"path": "docs/stories/feedback.md", "name": "Story: Article Feedback", "icon": "💬", "category": "workspace"},
        {"path": "docs/stories/hierarchy.md", "name": "Story: Article Hierarchy Integrity", "icon": "🌲", "category": "workspace"},
        {"path": "docs/stories/search.md", "name": "Story: Searching the Manual", "icon": "🔍", "category": "workspace"},
        {"path": "docs/stories/toc.md", "name": "Story: Dynamic Table of Contents", "icon": "📋", "category": "workspace"},
        {"path": "docs/stories/url_generation.md", "name": "Story: Dynamic URL Generation", "icon": "🔗", "category": "workspace"},
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
