# SPDX-License-Identifier: AGPL-3.0-or-later
{
    'name': 'Hams Test Companions (test_depends)',
    'version': '1.0',
    'category': 'Hidden/Tools',
    'summary': 'Install the modules a module\'s tests need, named in its manifest test_depends key, in plain --test-enable runs',
    'description': 'Reads the test_depends manifest key of every module being installed or upgraded in a --test-enable run, '
                   'refuses unknown, redundant, self-referential and cyclic entries, and marks the listed companion modules '
                   'to install so the tests find them. Needs no runner of its own.',
    'author': 'Hams',
    'depends': ['base'],
    'data': [],
    'post_load': 'install_patches',
    'installable': True,
    'application': False,
    'auto_install': False,
    'license': 'AGPL-3',
}
