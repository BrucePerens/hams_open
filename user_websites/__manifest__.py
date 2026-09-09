# SPDX-License-Identifier: AGPL-3.0-or-later
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
        "knowledge",
        "compliance",
        "cloudflare",
    ],
    "external_dependencies": {
        "python": ["markupsafe"],
    },
    "data": [
        # Security
        "security/user_websites_security.xml",
        "security/gdpr_export_security.xml",
        "security/ir.model.access.csv",
        # Data
        "data/user_websites_data.xml",
        "data/procedures.xml",
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
            "category": "workspace",
            # Bug-hunt fix (2026-09-09): this doc is the target of every
            # "help" link scattered across the module's own end-user-facing
            # pages (account suspension notices, GDPR export/erasure,
            # community directory, report-violation modal, the personal-site
            # welcome header -- see user_websites_templates.xml's own
            # #UX_* anchors) and controllers/main.py's own /user-websites/
            # documentation route, all reachable by ordinary portal/public
            # users, not just internal staff. Without "public": True, this
            # entry bootstraps as an INTERNAL-only knowledge.article
            # (is_published=False) -- knowledge/controllers/main.py's own
            # manual_article_view then 404s it for exactly that audience
            # (`if not is_internal and not article.is_published: ... 404`),
            # even though controllers/main.py's own documentation() route
            # already found and redirected to it via a rules-respecting
            # search(). Confirmed live and reproducible: every real request
            # to the redirect target 404's for a non-internal test user,
            # which is what test_08_frontend_misc_tour's own repeated,
            # reproducible failure (waiting forever for a page that never
            # rendered) actually was -- not a symptom of the unrelated
            # cross-session Chrome-kill bug this was first attributed to.
            "public": True,
        }
    ],
    "assets": {
        "web.assets_frontend": [
            "user_websites/static/src/js/violation_report.js",
            "user_websites/static/src/js/toast_notifications.js",
        ],
        "web.assets_tests": [
            "user_websites/static/tests/tours/backend_views_tour.js",
            "user_websites/static/tests/tours/community_directory_tour.js",
            "user_websites/static/tests/tours/create_blog_tour.js",
            "user_websites/static/tests/tours/create_site_tour.js",
            "user_websites/static/tests/tours/frontend_misc_tour.js",
            "user_websites/static/tests/tours/gdpr_privacy_tour.js",
            "user_websites/static/tests/tours/moderation_appeal_tour.js",
            "user_websites/static/tests/tours/toast_notifications_tour.js",
            "user_websites/static/tests/tours/violation_report_tour.js",
        ],
        "web.assets_unit_tests": [
            # Both source files also have to be listed here, not just in
            # web.assets_frontend above: /web/tests's own
            # assets_unit_tests_setup bundle only ('include's)
            # web.assets_backend, never web.assets_frontend -- same gap
            # hams_com/ham_shack/__manifest__.py's own sdr_spectrum.js
            # comment documents for the identical reason.
            "user_websites/static/src/js/violation_report.js",
            "user_websites/static/tests/violation_report.test.js",
            "user_websites/static/src/js/toast_notifications.js",
            "user_websites/static/tests/toast_notifications.test.js",
        ],
    },
    "demo": [],
    "installable": True,
    "application": True,
    "post_init_hook": "post_init_hook",
}
