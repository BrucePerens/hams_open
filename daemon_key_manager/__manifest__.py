{
    "name": "Daemon Key Manager",
    "version": "1.0",
    "summary": "Generalized, Open Source API Key Vault and File Writer for External Daemons",
    "category": "Security",
    "author": "Bruce Perens K6BP",
    "license": "AGPL-3",
    "depends": ["base", "zero_sudo", "hams_test"],
    "data": [
        "security/security.xml",
        "security/ir.model.access.csv",
        "data/cron.xml",
        "views/registry_views.xml",
    ],
    "assets": {
        "web.assets_tests": [
            "daemon_key_manager/static/src/js/tours/daemon_key_manager_tour.js",
        ],
    },
    "knowledge_docs": [
        {
            # [@ANCHOR: documentation_installed]
            "name": "Daemon Key Manager Documentation",
            "path": "data/documentation.html",
            "icon": "🔑",
            "category": "workspace"
        }
    ],
    "installable": True,
    "application": False,
}
