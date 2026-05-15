# -*- coding: utf-8 -*-
{
    "name": "Backup Management",
    "summary": "Unified Backup Management Facility (Kopia & pgBackRest)",
    "author": "Bruce Perens K6BP",
    "category": "Ham Radio",
    "license": "AGPL-3",
    "version": "1.0",
    "depends": ["base", "mail", "zero_sudo", "hams_test", "binary_downloader", "pager_duty"],
    "external_dependencies": {
        "python": ["pika", "cryptography"],
    },
    "data": [
        "security/security.xml",
        "security/ir.model.access.csv",
        "data/cron.xml",
        "views/backup_config_views.xml",
        "views/restore_wizard_views.xml",
        "views/backup_snapshot_views.xml",
        "views/backup_job_views.xml",

        "views/backup_board_views.xml",
        "views/menu_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Backup Management",
            "path": "data/documentation.html",
            "icon": "💾",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_backend": [
            "backup_management/static/src/components/board/board.js",
            "backup_management/static/src/components/board/board.xml",
        ],
        "web.assets_tests": [
            "backup_management/static/src/tests/**/*",
        ],
    },
}
