# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

{
    "name": "Edge Routing & Resolution",
    "summary": "Core foundational module for vanity URL resolution and custom domain routing.",
    "description": "This module provides high-speed slug caching and edge routing capabilities to decouple vanity URLs from heavy modules.",
    "version": "1.0",
    "category": "Website",
    "author": "HAMS",
    "depends": ["base", "distributed_redis_cache", "zero_sudo", "mail"],
    "external_dependencies": {
        "python": ["requests"],
    },
    "data": [
        "data/security_data.xml",
        "security/ir.model.access.csv",
    ],
    "installable": True,
    "application": False,
    "license": "AGPL-3",
}
