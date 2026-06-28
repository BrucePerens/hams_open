# -*- coding: utf-8 -*-
{
    "name": "Edge Routing & Resolution",
    "summary": "Core foundational module for vanity URL resolution and custom domain routing.",
    "description": "This module provides high-speed slug caching and edge routing capabilities to decouple vanity URLs from heavy modules.",
    "version": "1.0",
    "category": "Website",
    "author": "HAMS",
    "depends": ["base", "distributed_redis_cache", "zero_sudo"],
    "data": [
        "data/security_data.xml",
        "security/ir.model.access.csv",
    ],
    "installable": True,
    "application": False,
    "license": "OEEL-1",
}
