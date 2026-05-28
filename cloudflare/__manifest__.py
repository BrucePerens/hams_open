{
    "name": "Cloudflare Edge Orchestration",
    "summary": "Generalized CDN Edge Orchestration, Proactive Purging, and WAF Management.",
    "author": "Open Source Community",
    "category": "Website",
    "version": "1.3",
    "license": "AGPL-3",
    "depends": ["base", "zero_sudo", "website", "website_blog", "website_sale"],
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
    ],
    "knowledge_docs": [
        {
            "name": "Cloudflare Edge Orchestration Documentation",
            "path": "data/documentation.html",
            "icon": "☁️",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_tests": [
            "cloudflare/static/tests/tours/**/*",
        ],
    },
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
}
