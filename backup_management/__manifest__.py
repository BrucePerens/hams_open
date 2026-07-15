# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0-or-later

{
    "name": "Backup Management",
    "summary": "Centralized daemon management for pgBackRest and Kopia",
    "description": "Unified Backup Management Facility (Kopia & pgBackRest).",
    "author": "Bruce Perens K6BP",
    "category": "Administration",
    "license": "AGPL-3",
    "version": "1.0",
    "depends": [
        "base",
        "mail",
        "zero_sudo",
        "binary_downloader",
        "pager_duty",
        "website",
        "knowledge",
        "hams_rabbitmq",
        "daemon_key_manager",
    ],
    "external_dependencies": {
        "python": ["cryptography", "pika"],
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
    ],  # [@ANCHOR: backup_management:COMM_backup_doc_injection]
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
