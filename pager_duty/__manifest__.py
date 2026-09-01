# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
{
    "name": "Pager Duty",
    "summary": "Pager duty scheduling and incident management.",
    "description": "Pager duty scheduling and incident management.",
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
        "hams_helpdesk",
        "website",
        "knowledge",
    ],
    # Not a real Odoo manifest key -- see
    # hams_shared/tools/check_dependency_cycles.py. pager_check.py's
    # rpc_ensure_executable() optionally uses binary_downloader (to
    # auto-provision monitoring tool binaries), but binary_downloader
    # itself depends on pager_duty, so a real 'depends' entry here would
    # close that loop. Resolved at runtime via
    # zero_sudo.security.utils._resolve_dependency_cycle("binary_downloader").
    "depends_cycle": ["binary_downloader"],
    "external_dependencies": {
        # "mcp" is for daemon/pager_mcp_server.py -- PAGER_DUTY_MCP_AI_TRIAGE.md's
        # real build order slice 1, same library hams_shared/tools/mcp_watchdog.py
        # already depends on for its own FastMCP server.
        "python": ["psutil", "redis", "mcp"],
    },
    "data": [
        "security/security.xml",
        "data/cron.xml",
        "data/mail_alias_data.xml",
        "security/ir.model.access.csv",
        "views/incident_views.xml",
        "views/schedule_views.xml",
        "views/pager_check_views.xml",
        "views/log_analyzer_views.xml",
        "views/board_templates.xml",
        "data/procedures.xml",
        "data/pager_log_monitoring_data.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Pager Duty & Generalized Monitoring",
            "path": "data/documentation.html",
            "icon": "📟",
            "category": "workspace",
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
            "pager_duty/static/src/tours/pager_check_tour.js",
        ],
    },
}
