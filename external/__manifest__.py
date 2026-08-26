# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Proprietary, Trade-Secret.
{
    "name": "External Dependencies",
    "version": "1.0",
    "author": "Bruce Perens K6BP",
    "category": "Hidden",
    "summary": "Local hosting of external libraries for isolated networks.",
    "description": """
        Local hosting of external libraries for isolated networks.
    """,
    "license": "AGPL-3",
    "depends": ["zero_sudo", "base", "web"],
    # No "assets" bundle keys here on purpose. This module's whole job is
    # "Local hosting of external libraries for isolated networks" -- the
    # vendored files stay unminified and are reached via Odoo's ordinary
    # generic static-file route (/external/static/src/node_modules/...,
    # see [@ANCHOR: external:HTTP_REACHABLE_LEAFLET] and [@ANCHOR: external:HTTP_REACHABLE_TRANSFORMERS]
    # in tests/test_assets.py),
    # never via t-call-assets. transformers.js in particular contains
    # nested ES6 template literals; declaring either file under an
    # "assets" bundle key would make Odoo re-minify it with rjsmin, whose
    # own docs admit it only supports "(Unnested) template literals" --
    # this is exactly the mechanism behind the systemic "Uncaught
    # SyntaxError: Unexpected identifier 'Unexpected'" browser-tour
    # failures traced and fixed in hams_com commit f1f00511 (a different,
    # separately-vendored copy of the same library). Two now-dead
    # "external.assets_leaflet" / "external.assets_transformers" bundle
    # keys used to live here, unreferenced by any t-call-assets anywhere
    # in either repo -- pure landmine, no purpose. Removed rather than
    # left as a trap for a future module to unknowingly wire in.
    "knowledge_docs": [
        {
            "name": "External Dependencies",
            "path": "data/documentation.html",
            "icon": "📦",
            "category": "workspace",
        }
    ],
    "installable": True,
    "auto_install": False,
}
