# -*- coding: utf-8 -*-
{
    "name": "HAMS Base",
    "summary": "Core email routing, policies, and foundation for HAMS",
    "description": "Handles email compliance, bounce intelligence, and base policy.",
    "author": "Bruce Perens K6BP",
    "category": "Base",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": ["base", "mail", "website", "compliance"],
    "data": [
        "views/email_policy_template.xml",
        "views/res_config_settings_views.xml",
        "views/dmarc_report_views.xml",
        "security/ir.model.access.csv",
        "views/unsubscribe_templates.xml",
        "views/mail_templates.xml",
        "data/compliance_document_data.xml",
    ],
    "installable": True,
    "auto_install": False,
}
