# This software is distributed under the terms of the Affero General Public License (AGPL-3).

"""
This is the __init__.py file for the 'i18n' sub-package.

The 'i18n' directory (short for internationalization) is the standard place in
Odoo modules to store translation files. These files, typically with a .po
extension, contain the mappings of source terms (in English) to their
translations in other languages.

While this `__init__.py` file is currently empty, its presence is important.
It makes the 'i18n' directory a Python package. Although Odoo's translation
mechanism doesn't rely on this file being populated, it's good practice to
include it for consistency and to adhere to standard Python package structures.

Odoo automatically scans this directory for .po files and loads them when a
language is installed or updated, making the module's interface available in
multiple languages.
"""
