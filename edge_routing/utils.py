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
}


def slugify(s, max_length=None):
    # [@ANCHOR: edge_routing:utils_slugify]

    # Verified by [@ANCHOR: test_utils_slugify]
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
    if max_length:
        s = s[:max_length].rstrip("-")
    return s
