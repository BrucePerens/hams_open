{
    "name": "Hams Helpdesk",
    "version": "1.1",
    "category": "Operations/Helpdesk",
    "summary": "Zero-Sudo compliant, lightweight helpdesk management.",
    "description": "Zero-Sudo compliant, lightweight helpdesk management.",
    "author": "Bruce Perens K6BP",
    "depends": [
        "knowledge",
        "base",
        "mail",
        "calendar",
        "portal",
        "zero_sudo",
        "website",
        "ham_callbook",
        "daemon_key_manager",
        "distributed_redis_cache",
    ],
    "data": [
        "security/helpdesk_security.xml",
        "security/mail_ingest_security.xml",
        "security/ir.model.access.csv",
        "data/mail_alias_data.xml",
        "data/ncmec_report_data.xml",
        "views/helpdesk_ticket_views.xml",
        "views/shift_handoff_views.xml",
        "views/exception_fingerprint_views.xml",
        "views/dashboard_views.xml",
        "views/portal_templates.xml",
        "data/member_menu_data.xml",
    ],
    "assets": {
        # The browser JavaScript error reporter (docs/stories/server_exception_tickets.md, section 13). Website,
        # portal and back office only; the relay-served console does not load these bundles.
        "web.assets_frontend": [
            "hams_helpdesk/static/src/js/browser_error_reporter.js",
            "hams_helpdesk/static/src/js/browser_error_reporter_boot.js",
        ],
        "web.assets_backend": [
            "hams_helpdesk/static/src/js/browser_error_reporter.js",
            "hams_helpdesk/static/src/js/browser_error_reporter_boot.js",
        ],
        "web.assets_unit_tests": [
            "hams_helpdesk/static/src/js/browser_error_reporter.js",
            "hams_helpdesk/static/tests/browser_error_reporter.test.js",
        ],
        "web.assets_tests": [
            "hams_helpdesk/static/src/tours/helpdesk_operator_tour.js",
            "hams_helpdesk/static/src/tours/helpdesk_portal_tour.js",
        ],
    },
    "knowledge_docs": [
        {
            # The member-facing manual for the Support menu item (/my/tickets). It is
            # the only member entry here: "Hams Helpdesk" below is developer
            # documentation and stays internal.
            "name": "Getting help: Support tickets",
            "path": "data/member_manual.html",
            "icon": "🎫",
            "category": "workspace",
            "public": True,
            "help_topic": "Getting started and your account",
        },
        {
            "name": "Hams Helpdesk",
            "path": "data/documentation.html",
            "icon": "🎫",
            "category": "workspace",
        },
        {
            "name": "Story: Helpdesk Ticket Lifecycle",
            "path": "docs/stories/helpdesk_ticket_lifecycle.md",
            "icon": "🔄",
            "category": "workspace",
        },
        {
            "name": "Story: Helpdesk Ticket Creation",
            "path": "docs/stories/helpdesk_ticket_creation.md",
            "icon": "🆕",
            "category": "workspace",
        },
        {
            "name": "Journey: Incident Resolution",
            "path": "docs/journeys/incident_resolution.md",
            "icon": "🛠️",
            "category": "workspace",
        },
        {
            "name": "Journey: Shift Handoff Protocol",
            "path": "docs/journeys/shift_handoff_protocol.md",
            "icon": "🤝",
            "category": "workspace",
        },
        {
            "name": "Story: Automatic Server-Exception Tickets",
            "path": "docs/stories/server_exception_tickets.md",
            "icon": "🐞",
            "category": "workspace",
        },
        {
            "name": "Story: Multi-Website Segregation",
            "path": "docs/stories/multi_website_segregation.md",
            "icon": "🌐",
            "category": "workspace",
        },
    ],
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": True,
    "license": "AGPL-3",
}
