# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
{
    "name": "Cloudflare Edge Orchestration",
    "summary": "Generalized CDN Edge Orchestration, Proactive Purging, and WAF Management.",
    "description": "Generalized CDN Edge Orchestration, Proactive Purging, and WAF Management.",
    "author": "Open Source Community",
    "category": "Website",
    "version": "1.3",
    "license": "AGPL-3",
    "depends": [
        "base",
        "zero_sudo",
        "website",
        "website_blog",
        "website_sale",
        "edge_routing",
        "knowledge",
    ],
    "data": [
        "security/security_data.xml",
        "security/ir.model.access.csv",
        "data/cron.xml",
        "views/tunnel_wizard_views.xml",
        "views/res_config_settings_views.xml",
        "views/cloudflare_menus.xml",
        "views/ip_ban_views.xml",
        "views/waf_rule_views.xml",
        "views/config_backup_views.xml",
        "views/purge_wizard_views.xml",
        "views/zone_settings_wizard_views.xml",
        "views/tunnel_views.xml",
        "views/cloudflare_features_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Cloudflare Edge Orchestration Documentation",
            "path": "data/documentation.html",
            "icon": "☁️",
            "category": "workspace",
        }
    ],
    "assets": {
        "web.assets_backend": [
            "cloudflare/static/src/components/analytics/analytics.js",
            "cloudflare/static/src/components/analytics/analytics.xml",
        ],
        "web.assets_tests": [
            "cloudflare/static/tests/tours/ip_ban_tour.js",
            "cloudflare/static/tests/tours/purge_wizard_tour.js",
            "cloudflare/static/tests/tours/waf_rule_tour.js",
            "cloudflare/static/tests/tours/zone_settings_tour.js",
        ],
    },
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
}
