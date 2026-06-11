# -*- coding: utf-8 -*-
{
    "name": "Backup Management",
    "summary": "Unified Backup Management Facility (Kopia & pgBackRest)",
    "description": "Unified Backup Management Facility (Kopia & pgBackRest).",
    "author": "Bruce Perens K6BP",
    "category": "Ham Radio",
    "license": "AGPL-3",
    "version": "1.0",
    "depends": [
        "base",
        "mail",
        "zero_sudo",
        "binary_downloader",
        "pager_duty",
        "website",
    ],
    "external_dependencies": {
        "python": [],
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
    "post_init_hook": "post_init_hook",
    "knowledge_docs": [
        {
            "name": "Backup Management",
            "path": "data/documentation.html",
            "icon": "💾",
            "category": "workspace",
        }
    ],  # [@ANCHOR: backup_doc_injection]
    "assets": {
        "web.assets_backend": [
            "backup_management/static/src/components/board/board.js",
            "backup_management/static/src/components/board/board.xml",
        ],
        "web.assets_unit_tests": [
             "backup_management/static/src/components/board/board.js",
        ],
        "web.assets_tests": [
            "backup_management/static/src/tests/tours/backup_dashboard.js",
        ],
    },
}
