# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "HAMS RPC Guard",
    "version": "1.0",
    "category": "Hidden/Tools",
    "summary": "Odoo's generic RPC endpoints (XML-RPC, JSON-RPC, JSON-2) answer only allowed networks (default: this machine) or a Unix socket",
    "description": """
Refuses /xmlrpc, /xmlrpc/2/*, /jsonrpc and /json/2/* to a caller whose true client is not on an allowed network
(default: loopback only), believing a forwarding header only from a trusted proxy, and can serve them only on a Unix
socket whose callers the kernel identifies (SO_PEERCRED). Limits which model methods each caller may use over RPC.
Safe by default on a stock Odoo 19 Community server: nothing but Odoo core and zero_sudo is needed.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "license": "AGPL-3",
    "depends": ["base", "rpc", "zero_sudo", "knowledge"],
    "data": [],
    "knowledge_docs": [
        {
            "name": "RPC Guard",
            "path": "data/documentation.html",
            "icon": "R",
            "category": "workspace",
        }
    ],
    "post_init_hook": "post_init_hook",
    "post_load": "post_load_hook",
    "installable": True,
    "application": False,
    "auto_install": False,
}
