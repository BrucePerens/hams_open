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
import weakref

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
    # 2026-10-13: the training area moved from /auxcomm/... to /training/... (the old words stay reserved: they redirect).
    "training",
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
    "picture-credits",
    "s",
    "survey",
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
    # Added 2026-10-13: first segments of platform routes that this list lacked (found by the guard test in
    # a run with the hams_com modules loaded, and by listing hams_prod's live routing map). A member who
    # had claimed one would own a site nobody could reach. reserved_slug_words() now also reads the live
    # routing map, so a route added later is covered even before it is written here.
    "calendar",
    "ca",
    "communications-history",
    "ics",
    "lead",
    "legal",
    "mailing",
    "nic",
    "pager",
    "profile",
    "r",
    "request-invite",
    "responsible-messages",
    "silent-key-policy",
    "simulated-band",
    "view",
    "webhook",
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


def language_slugs(env):
    """Every first path segment Odoo's own language handling owns: for each installed language its
    code (en_US), its URL code (en, fr) and the bare language prefix (fr for fr_FR), lower-cased.

    A site slug equal to one of these would shadow Odoo's /<lang>/... routing ("/en_US/" is the
    home page in English), and a route that matched it would 404 the language prefix.
    """
    # [@ANCHOR: edge_routing:language_slugs]
    # Verified by [@ANCHOR: test_language_codes_are_not_slugs]
    words = set()
    langs = env["res.lang"]
    for code, _name in langs.get_installed():
        words.add(code.lower())
        words.add(code.split("_")[0].lower())
        url_code = langs._get_data(code=code).url_code
        if url_code:
            words.add(url_code.lower())
    return words


# First path segments per routing map object. The map is rebuilt whenever Odoo invalidates its routing cache, and a
# weak key lets an old map (and its entry) go with it, so an entry is never stale.
_FIRST_SEGMENTS_BY_ROUTING_MAP = weakref.WeakKeyDictionary()


def routed_first_segments(env):
    """Every fixed first path segment of a route registered in this database's routing map, lower-cased.

    A static route wins over the /<slug> converter route, so a member or group that claimed one of
    these words would own a site nobody could reach, and a platform route added next year would
    silently shadow a slug already claimed. Reading the live routing map (the one the server routes
    with) keeps RESERVED_SLUGS from having to be updated by hand for each new route.
    """
    # [@ANCHOR: edge_routing:routed_first_segments]
    # Verified by [@ANCHOR: test_routed_first_segments_are_reserved]
    if not env.registry.loaded:
        # While modules are still installing the routing map would be built from the modules loaded so
        # far and cached half-made; the data a module ships is covered by the fixed list and the tests.
        return frozenset()
    routing_map = env["ir.http"].routing_map()
    # The map object is rebuilt whenever Odoo invalidates its routing cache, so a result kept for it
    # is never stale.
    words = _FIRST_SEGMENTS_BY_ROUTING_MAP.get(routing_map)
    if words is None:
        words = set()
        for rule in routing_map.iter_rules():
            parts = [part for part in rule.rule.split("/") if part]
            if parts and "<" not in parts[0] and parts[0] != "static":
                words.add(parts[0].lower())
        words = frozenset(words)
        _FIRST_SEGMENTS_BY_ROUTING_MAP[routing_map] = words
    return words


def reserved_slug_words(env):
    """Every word a member or group site slug may not be: the fixed list, the first segment of any
    registered route, and Odoo's language prefixes."""
    # [@ANCHOR: edge_routing:reserved_slug_words]
    # Verified by [@ANCHOR: test_routed_first_segments_are_reserved]
    return RESERVED_SLUGS | routed_first_segments(env) | language_slugs(env)
