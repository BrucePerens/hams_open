# SPDX-License-Identifier: AGPL-3.0-or-later
{
    'name': 'SES Webhook Receiver',
    'version': '1.0',
    'summary': 'Receives HTTP webhooks from smtp2http for Amazon SES inbound emails',
    'description': 'A module to securely receive Amazon SNS webhooks containing SES incoming emails.',
    'author': 'HAMS',
    'category': 'Mail',
    'depends': ['mail'],
    'data': [
        'security/ir.model.access.csv',
        'views/ses_webhook_views.xml',
        'data/ir_cron.xml',
    ],
    'installable': True,
    'application': False,
    'license': 'AGPL-3',
}
