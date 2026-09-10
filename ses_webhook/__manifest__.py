# SPDX-License-Identifier: AGPL-3.0-or-later
{
    'name': 'SES Webhook Receiver',
    'version': '1.0',
    'summary': 'Receives HTTP webhooks from smtp2http for Amazon SES inbound emails',
    'description': 'A module to securely receive Amazon SNS webhooks containing SES incoming emails.',
    'author': 'HAMS',
    'category': 'Mail',
    'depends': ['mail', 'zero_sudo'],
    # 'cryptography' verifies real AWS SNS message signatures
    # (controllers/webhook_api.py's _verify_sns_signature) -- already
    # guaranteed present (Odoo itself depends on it), declared here for
    # accuracy/discoverability, not because installation actually needs it.
    'external_dependencies': {
        'python': ['cryptography'],
    },
    'data': [
        'security/ses_webhook_security.xml',
        'security/ir.model.access.csv',
        'data/security_backfill.xml',
        'views/ses_webhook_views.xml',
        'data/ir_cron.xml',
    ],
    'installable': True,
    'application': False,
    'license': 'AGPL-3',
}
