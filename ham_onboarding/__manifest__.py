{
    'name': 'Ham Onboarding',
    'description': 'Onboarding module for Ham amateur radio operators, including QRZ token generation and official OTP verification.',
    'version': '1.0',
    'category': 'Hidden',
    'depends': ['base', 'zero_sudo', 'mail'],
    'data': [
        'data/mail_template.xml',
    ],
    'installable': True,
    'auto_install': False,
    'license': 'AGPL-3',
    'author': 'Bruce Perens K6BP',
}
