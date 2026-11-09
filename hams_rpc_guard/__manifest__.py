# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "HAMS RPC Guard",
    "version": "1.0",
    "category": "Hidden/Tools",
    "summary": "Odoo's generic RPC endpoints (XML-RPC, JSON-RPC, JSON-2) answer only the machine they run on, by default",
    "description": """
Refuses /xmlrpc, /xmlrpc/2/*, /jsonrpc and (optionally) /json/2/* to a caller whose true client is not on the server
itself, looking through nginx and Cloudflare Tunnel (where the socket peer is always loopback) at the forwarding
headers. An operator can open or close each endpoint family with a system parameter. Safe by default on a stock
Odoo 19 Community server: nothing but Odoo core and zero_sudo is needed.
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
