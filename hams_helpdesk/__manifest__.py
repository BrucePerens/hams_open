{
    "name": "Hams Helpdesk",
    "version": "1.0",
    "category": "Operations/Helpdesk",
    "summary": "Zero-Sudo compliant, lightweight helpdesk management.",
    "author": "Bruce Perens K6BP",
    "depends": ["base", "mail", "calendar", "portal"],
    "data": [
        "security/helpdesk_security.xml",
        "security/ir.model.access.csv",
        "views/helpdesk_ticket_views.xml",
        "views/shift_handoff_views.xml",
        "views/dashboard_views.xml",
        "views/portal_templates.xml",
    ],
    "assets": {
        "web.assets_tests": [
            "hams_helpdesk/static/tests/tours/**/*",
        ],
    },
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": True,
    "license": "AGPL-3",
}
