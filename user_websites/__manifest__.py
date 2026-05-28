{
    "name": "User Websites",
    "summary": "Allow users to create personal or group websites and blogs.",
    "description": """
This module enables:
- Users to have a personal website (e.g. /username/home)
- Users to create shared group websites (e.g. /groupname/home)
- Users to manage a blog within their site.
- Privacy controls and content violation reporting.
- Advanced Moderation, 3-Strike suspension, and Appeals.
- Subscriptions and Privacy-Friendly View Counters.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "category": "Website",
    "version": "0.3",
    "license": "AGPL-3",
    "depends": [
        "base",
        "website",
        "website_blog",
        "mail",
        "portal",
        "zero_sudo",
        "distributed_redis_cache",
        "cloudflare",
    ],
    "external_dependencies": {
        "python": ["redis"],
    },
    "data": [
        # Security
        "security/user_websites_security.xml",
        "security/ir.model.access.csv",
        # Data
        "data/user_websites_data.xml",
        # Views
        "views/res_config_settings_views.xml",
        "views/res_users_views.xml",
        "views/user_websites_group_views.xml",
        "views/website_page_views.xml",
        "views/blog_post_views.xml",
        "views/content_violation_report_views.xml",
        "views/content_violation_appeal_views.xml",
        # Templates
        "views/user_websites_templates.xml",
        "views/website_layout.xml",
        "views/snippets.xml",
    ],
    "knowledge_docs": [
        {
            "name": "User Websites Documentation",
            "path": "data/documentation.html",
            "icon": "🌐",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_frontend": [
            "user_websites/static/src/js/violation_report.js",
            "user_websites/static/src/js/toast_notifications.js",
        ],
        "web.assets_tests": [
            "user_websites/static/tests/tours/**/*",
        ],
    },
    "demo": [],
    "installable": True,
    "application": True,
    "post_init_hook": "post_init_hook",
}
