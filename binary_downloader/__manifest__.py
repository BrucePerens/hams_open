{
    "name": "Binary Downloader",
    "summary": "Secure, DB-backed binary dependency provisioner",
    "version": "1.0",
    "category": "Hidden",
    "author": "Bruce Perens K6BP",
    "depends": ["base", "zero_sudo", "hams_test"],
    "data": [
        "security/security.xml",
        "security/ir.model.access.csv",
        "data/binary_manifest_data.xml",
        "views/binary_manifest_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Binary Downloader Facility",
            "path": "data/documentation.html",
            "icon": "📦",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_tests": [
            "binary_downloader/static/tests/**/*",
        ],
    },
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
    "license": "AGPL-3",
}
