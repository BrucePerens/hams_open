# SPDX-License-Identifier: AGPL-3.0-or-later
{
    'name': 'Hams S3 Config',
    'version': '1.0',
    'category': 'Hidden/Tools',
    'summary': 'Configure S3 Storage Backend directly from General Settings',
    'description': 'Configure S3 Storage Backend directly from General Settings',
    'author': 'Hams',
    'depends': ['base_setup'],
    'data': [
        'views/res_config_settings_views.xml',
    ],
    'installable': True,
    'application': False,
    'auto_install': False,
    'license': 'AGPL-3',
}
