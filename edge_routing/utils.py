# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

"""
Utility functions for the user_websites module.
"""

import unicodedata
import re

RESERVED_SLUGS = {
    "community",
    "blog",
    "website",
    "contactus",
    "aboutus",
    "forum",
    "shop",
    "my",
    "web",
    "qso",
    "qsl",
    "logbook",
    "shack",
    "dx",
    "ares",
    "arrl",
    "repeaters",
    "elmers",
    # Added 2026-10-12 with the move of every member site's home from /<slug>/home to /<slug>:
    # a member site now owns the single path segment, so any word that is already the first
    # segment of a platform route (static routes win over the /<slug> converter route and would
    # silently shadow the member's site) or a word that is a reserved sub-path of a member site
    # can no longer be claimed. test_reserved_slugs_cover_static_single_segment_routes keeps this
    # list in step with the live routing map.
    "home",
    "admin",
    "administrator",
    "api",
    "aboutus",
    "auxcomm",
    "backup",
    "bus",
    "chat",
    "classifieds",
    "cloudflare",
    "club",
    "clubs",
    "contact",
    "crawler",
    "demo",
    "digest",
    "discuss",
    "email-policy",
    "event",
    "events",
    "help",
    "helpdesk",
    "hams",
    "identity-ca",
    "invite",
    "json",
    "jobs",
    "knowledge",
    "live",
    "login",
    "logout",
    "lotw",
    "mail",
    "manual",
    "member",
    "members",
    "odoo",
    "onboarding",
    "page",
    "pages",
    "portal",
    "privacy",
    "propagation",
    "relay",
    "relay-ca",
    "robots.txt",
    "satellite",
    "settings",
    "signup",
    "sitemap.xml",
    "status",
    "static",
    "support",
    "unsubscribe",
    "upload",
    "user-websites",
    "var",
    "www",
    "account",
    "auth",
    "compliance",
    "dashboard",
    "delivery",
    "doc",
    "doc-bearer",
    "donation",
    "invoice",
    "jsonrpc",
    "kpi",
    "logo",
    "meet",
    "model",
    "offline",
    "partners",
    "payment",
    "product",
    "rate",
    "report",
    "sale",
    "sms",
    "snippets",
    "spreadsheet",
    "terms",
    "websocket",
    "xmlrpc",
}


def slugify(s, max_length=None):
    # [@ANCHOR: edge_routing:utils_slugify]

    # # Verified by [@ANCHOR: test_utils_slugify]
    """
    Transform a string to a slug.

    This is a local implementation to avoid dependencies on changing Odoo internals
    (specifically `odoo.addons.http_routing.models.ir_http.slugify` which moves
    between versions).

    Args:
        s (str): The string to slugify.
        max_length (int, optional): The maximum length of the slug.

    Returns:
        str: The slugified string.
    """
    if not s:
        return ""
    s = str(s).strip().lower()
    s = unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode("utf-8")
    s = re.sub(r"[^a-z0-9]+", "-", s)
    s = s.strip("-")
    if max_length is not None:
        # `if max_length:` would silently skip truncation for
        # max_length=0 (falsy), treating "truncate to nothing" as "don't
        # truncate at all" -- a caller building a slug length budget
        # dynamically could pass 0 and get the full untruncated slug back.
        s = s[:max_length].rstrip("-")
    return s
