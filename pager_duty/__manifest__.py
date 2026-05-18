# -*- coding: utf-8 -*-
{
    "name": "Pager Duty",
    "summary": "Pager duty scheduling and incident management.",
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "category": "Ham Radio",
    "post_init_hook": "post_init_hook",
    "license": "AGPL-3",
    "version": "1.0",
    "depends": [
        "base",
        "mail",
        "calendar",
        "bus",
        "zero_sudo",
        "distributed_redis_cache",
        "hams_test",
        "hams_helpdesk",
    ],
    "external_dependencies": {
        "python": ["redis", "psutil", "ntplib", "pymysql", "ldap3"],
    },
    "data": [
        "security/security.xml",
        "data/cron.xml",
        "security/ir.model.access.csv",
        "views/incident_views.xml",
        "views/schedule_views.xml",
        "views/pager_check_views.xml",
        "views/log_analyzer_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Pager Duty & Generalized Monitoring",
            "path": "data/documentation.html",
            "icon": "📟",
            "category": "workspace"
        }
    ],
    "assets": {
        "web.assets_backend": [
            "pager_duty/static/src/components/board/board.js",
            "pager_duty/static/src/components/board/board.xml",
            "pager_duty/static/src/components/log_viewer/log_viewer.js",
            "pager_duty/static/src/components/log_viewer/log_viewer.xml",
        ],
        "web.assets_tests": [
            "pager_duty/static/src/tours/incident_tour.js",
        ]
    },
}
