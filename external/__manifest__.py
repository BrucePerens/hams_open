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
    "assets": {
        "external.assets_leaflet": [
            # [@ANCHOR: external:HTTP_REACHABLE_LEAFLET]
            "external/static/src/node_modules/leaflet/leaflet.css",
            "external/static/src/node_modules/leaflet/leaflet.js",
        ],
        "external.assets_transformers": [
            # [@ANCHOR: external:HTTP_REACHABLE_TRANSFORMERS]
            "external/static/src/node_modules/transformers/transformers.js",
        ],
    },
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
