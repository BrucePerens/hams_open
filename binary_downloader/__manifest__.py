# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.

{
    "name": "Binary Downloader",
    "summary": "Secure, DB-backed binary dependency provisioner",
    "description": "Secure, DB-backed binary dependency provisioner.",
    "version": "1.0",
    "category": "Hidden",
    "author": "Bruce Perens K6BP",
    "depends": ["base", "zero_sudo", "website", "pager_duty"],
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
