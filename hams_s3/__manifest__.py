# SPDX-License-Identifier: AGPL-3.0-or-later
{
    'name': 'Hams S3 Config',
    'version': '1.0',
    'category': 'Hidden/Tools',
    'summary': 'Configure S3 Storage Backend directly from General Settings',
    'description': 'Configure S3 Storage Backend directly from General Settings',
    'author': 'Hams',
    'depends': ['base_setup', 'zero_sudo', 'daemon_key_manager'],
    'data': [
        'security/hams_s3_security.xml',
        'views/res_config_settings_views.xml',
    ],
    'post_init_hook': 'post_init_hook',
    'installable': True,
    'application': False,
    'auto_install': False,
    'license': 'AGPL-3',
}
