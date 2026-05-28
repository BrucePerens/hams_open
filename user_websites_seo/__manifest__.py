{
    "name": "User Websites SEO",
    "summary": "Lets users optimize their personal and group blogs for search engines.",
    "description": "Inherits website.seo.metadata onto user profiles to restore the QWeb SEO widget.",
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "category": "Website",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": [
        "base",
        "website",
        "user_websites",
        "zero_sudo",
        "web_tour",
    ],
    "data": [
        "views/res_users_views.xml",
        "views/user_websites_group_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "User Websites SEO Guide",
            "path": "data/documentation.html",
            "icon": "🔍",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_tests": [
            "user_websites_seo/static/src/js/**/*",
        ],
    },
    "installable": True,
    "application": False,
    "auto_install": True,
}
