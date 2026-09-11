# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

{
    "name": "Zero-Sudo Security Core",
    "summary": "Foundational security utilities, service account patterns, and web isolation.",
    "description": """Zero-Sudo Security Core foundational module.

""",
    "author": "Bruce Perens K6BP",
    "category": "Security",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": ["base", "web", "mail"],
    # distributed_redis_cache depends on zero_sudo (its own security-account/param-read
    # plumbing), so zero_sudo can't declare a real 'depends' entry back on it -- that
    # would close a real cycle Odoo's module loader can't install. models/ir_http.py's
    # `from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache`
    # is the actual coupling this documents; see
    # zero_sudo/models/security_utils.py's `_resolve_dependency_cycle` docstring and
    # hams_shared/tools/check_dependency_cycles.py for the established convention.
    "depends_cycle": ["distributed_redis_cache"],
    "external_dependencies": {
        "python": ["psycopg2", "requests"]
    },
    "assets": {
        "web.assets_backend": [
            "zero_sudo/static/src/components/security_dashboard/security_dashboard.js",
            "zero_sudo/static/src/components/security_dashboard/security_dashboard.xml",
        ],
        "web.assets_frontend": [
            # Shared generic offline-queueing (IndexedDB) helper -- see the file's own header
            # comment for why this lives here rather than in ham_shack (its original home) or
            # ics_forms. Consumed via `import { OfflineStore } from "@zero_sudo/js/offline_store"`.
            "zero_sudo/static/src/js/offline_store.js",
        ],
        "web.assets_tests": [
            "zero_sudo/static/src/js/tour_utils.js",
            "zero_sudo/static/src/js/tour_failure_dump.js",
            "zero_sudo/static/src/tours/zero_sudo_tour.js",
        ],
        # Fast, hardware-independent hoot unit tests -- distinct from the full-browser-tour
        # suite above. offline_store.js is duplicated here (not just referenced via
        # web.assets_frontend above) because /web/tests's own assets_unit_tests_setup bundle
        # only 'include's web.assets_backend, never web.assets_frontend -- confirmed directly
        # (odoo/addons/web/__manifest__.py) and already the established convention in this
        # codebase (see ham_shack/__manifest__.py's own web.assets_unit_tests comment on
        # sdr_spectrum.js for the identical reasoning).
        "web.assets_unit_tests": [
            "zero_sudo/static/src/js/offline_store.js",
            "zero_sudo/static/tests/offline_store.test.js",
        ],
    },
    "data": [
        "data/security_data.xml",
        "data/ir_cron.xml",
        "data/postgres_procedures.xml",
        "security/ir.model.access.csv",
        "security/ir_rule.xml",
        "views/res_users_views.xml",
        "views/security_log_views.xml",
        "data/noisy_table_data.xml",
        "views/noisy_table_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Zero-Sudo Security Core",
            "path": "data/documentation.html",
            "icon": "🛡️",
            "category": "workspace",
        },
        {
            "name": "Real Transaction Testing Facility Guide",
            "path": "data/testing_documentation.html",
            "icon": "🧪",
            "category": "workspace",
        },
        {
            "name": "Developer Integration Journey",
            "path": "docs/journeys/developer_integration.md",
            "icon": "🚀",
            "category": "workspace",
        },
        {
            "name": "Multi-Website Security Story",
            "path": "docs/stories/multi_website.md",
            "icon": "🌐",
            "category": "workspace",
        },
        {
            "name": "High-Performance Atomic KV Storage",
            "path": "docs/stories/set_kv_procedure.md",
            "icon": "⚡",
            "category": "workspace",
        },
    ],
    "installable": True,
    "auto_install": False,
}
