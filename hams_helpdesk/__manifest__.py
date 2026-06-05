{
    "name": "Hams Helpdesk",
    "version": "1.0",
    "category": "Operations/Helpdesk",
    "summary": "Zero-Sudo compliant, lightweight helpdesk management.",
    "author": "Bruce Perens K6BP",
    "depends": [
        "manual_library", "base", "mail", "calendar", "portal", "zero_sudo", "website"],
    "data": [
        "security/helpdesk_security.xml",
        "security/ir.model.access.csv",
        "views/helpdesk_ticket_views.xml",
        "views/shift_handoff_views.xml",
        "views/dashboard_views.xml",
        "views/portal_templates.xml",
    ],
    "assets": {
    },
    "knowledge_docs": [
        {
            "name": "Hams Helpdesk",
            "path": "data/documentation.html",
            "icon": "🎫",
            "category": "technical",
        },
        {
            "name": "Story: Helpdesk Ticket Lifecycle",
            "path": "docs/stories/helpdesk_ticket_lifecycle.md",
            "icon": "🔄",
            "category": "technical",
        },
        {
            "name": "Story: Helpdesk Ticket Creation",
            "path": "docs/stories/helpdesk_ticket_creation.md",
            "icon": "🆕",
            "category": "technical",
        },
        {
            "name": "Journey: Incident Resolution",
            "path": "docs/journeys/incident_resolution.md",
            "icon": "🛠️",
            "category": "technical",
        },
        {
            "name": "Journey: Shift Handoff Protocol",
            "path": "docs/journeys/shift_handoff_protocol.md",
            "icon": "🤝",
            "category": "technical",
        },
        {
            "name": "Story: Multi-Website Segregation",
            "path": "docs/stories/multi_website_segregation.md",
            "icon": "🌐",
            "category": "technical",
        }
    ],
    "installable": True,
    "application": True,
    "license": "AGPL-3",
}
