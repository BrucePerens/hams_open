# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

{
    "name": "Binary Downloader",
    "summary": "Secure, DB-backed binary dependency provisioner",
    "description": "Secure, DB-backed binary dependency provisioner.",
    "version": "1.0",
    "category": "Hidden",
    "author": "Bruce Perens K6BP",
    "depends": ["base", "zero_sudo", "website", "pager_duty", "knowledge"],
    "data": [
        "security/security.xml",
        "security/ir.model.access.csv",
        "data/binary_manifest_data.xml",
        "views/binary_manifest_views.xml",
        "views/tenant_binary_views.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Binary Downloader Facility",
            "path": "data/documentation.html",
            "icon": "📦",
            "category": "workspace",
        }
    ],
    "assets": {
        "web.assets_tests": [
            "binary_downloader/static/tests/tours/binary_install_tour.js",
        ],
    },
    "installable": True,
    "application": False,
    "post_init_hook": "_post_init_hook",
    "license": "AGPL-3",
}
