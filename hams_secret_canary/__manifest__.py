# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "HAMS Secret Canary",
    "version": "1.0",
    "category": "Hidden/Tools",
    "summary": "Honeytoken credentials and a detector that raises an alarm when a canary or a registered secret appears in a response or a log line",
    "description": """
Plants decoy credentials (generated per install) where a bug-hunter or attacker looks first, and raises an alarm when a
decoy, or the fingerprint of any secret registered with the module, shows up in an HTTP response body or a log line.
Detects a leak after it happens; it does not prevent one. Needs nothing but Odoo core and zero_sudo: alerts go to the
Odoo log, a security-log row and an email to the administrator.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "license": "AGPL-3",
    "depends": ["base", "mail", "zero_sudo"],
    "data": [],
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
    "auto_install": False,
}
