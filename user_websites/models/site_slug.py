# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
"""One site per member: how a slug is chosen, and what moves when it changes.

[@ANCHOR: user_websites:site_slug_module]
Verified by [@ANCHOR: test_site_slug_one_site]

A site is every website.page whose url is /<slug> or /<slug>/..., plus the blog under /<slug>/blog.
A slug rename used to add a 301 from /<old> to /<new> and leave the pages at /<old>. Odoo serves a
published page before it looks at website.rewrite, so /<old> kept answering 200 as a second site, and
the owner's toolbar (which links by the CURRENT slug) pointed at pages whose url is not under that
slug: every "Edit this page" and "Unpublish" answered 404 (Bruce, 2026-10-13, K6BP).
rehome_site() runs on every rename, for members and groups alike: it moves the pages and menu items
to the new slug, keeps a page that would collide (unpublished, content retained), flattens redirect
chains and removes any redirect that would now shadow the new site.

SLUG_POLICY_PARAM names the policy. "name_callsign" (the default, the open-source behaviour) builds a
slug from the record's name. "callsign" (hams.com sets it from its own module data) builds it from the
member's callsign, see ResUsers._slug_identity().
"""
import logging
import re

from odoo import _
from odoo.exceptions import ValidationError

_logger = logging.getLogger(__name__)

SLUG_POLICY_PARAM = "user_websites.slug_policy"
POLICY_NAME_CALLSIGN = "name_callsign"
POLICY_CALLSIGN = "callsign"
SVC = "user_websites.user_websites_service_account"
_SLUG_PATH_RE = re.compile(r"^/[a-z0-9\-]+$")


def _like_escape(text):
    return text.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")


def slug_policy(env):
    """The deployment's slug policy; anything but "callsign" is the open-source default."""
    # [@ANCHOR: user_websites:fn_slug_policy]
    value = env["zero_sudo.security.utils"]._get_system_param(SLUG_POLICY_PARAM, POLICY_NAME_CALLSIGN)
    return POLICY_CALLSIGN if value == POLICY_CALLSIGN else POLICY_NAME_CALLSIGN


def legacy_slug_target(env, slug):
    """The current slug a retired slug redirects to, or None.

    The redirect rows website.rewrite already holds are the only record of a retired slug: a 301
    from /<old> to a single-segment /<new> that a member or group holds today. Chains are followed
    (three hops at most, no loops) so rows written before rehome_site() flattened them still lead
    to the live site.
    """
    # [@ANCHOR: user_websites:fn_legacy_slug_target]
    svc = env["zero_sudo.security.utils"]._get_service_env(SVC)
    current = (slug or "").lower()
    seen = {current}
    for _hop in range(3):
        row = svc["website.rewrite"].search(
            [("url_from", "=", f"/{current}"), ("redirect_type", "=", "301"), ("active", "=", True)], limit=1
        )
        if not row or not _SLUG_PATH_RE.match(row.url_to or ""):
            return None
        current = row.url_to[1:]
        if current in seen:
            return None
        seen.add(current)
        if svc["res.users"].get_record_by_slug(current) or svc["user.websites.group"].get_record_by_slug(current):
            return current
    return None


def assert_slug_free(env, slug, record):
    """Raise unless no OTHER member or group holds `slug` (they share one namespace)."""
    # [@ANCHOR: user_websites:fn_assert_slug_free]
    for model in ("res.users", "user.websites.group"):
        domain = [("website_slug", "=", slug)]
        if record is not None and record._name == model and record.id:
            domain.append(("id", "!=", record.id))
        if _count(env, model, domain):
            raise ValidationError(_("The site address '%s' is already taken.") % slug)


def _count(env, model, domain):
    svc = env["zero_sudo.security.utils"]._get_service_env("edge_routing.edge_routing_service_account")
    return svc[model].with_context(active_test=False).search_count(domain)


def rehome_site(env, owner_field, owner_id, old, new):
    """Move a renamed site's pages and menu items from /<old> to /<new>; return a log of the moves."""
    # [@ANCHOR: user_websites:fn_rehome_site]
    # Verified by [@ANCHOR: test_site_slug_one_site]
    log = []
    if not old or not new or old == new:
        return log
    svc = env["zero_sudo.security.utils"]._get_service_env(SVC)
    rewrite = svc["website.rewrite"]

    # A redirect whose source is the slug being claimed would shadow the new site; one that points at
    # the slug being left would chain through it.
    shadow = rewrite.search(["|", ("url_from", "=", f"/{new}"), ("url_from", "=like", _like_escape(f"/{new}/") + "%")])
    if shadow:
        log.append(("drop_redirect", shadow.mapped("url_from")))
        shadow.unlink()
    for row in rewrite.search(["|", ("url_to", "=", f"/{old}"), ("url_to", "=like", _like_escape(f"/{old}/") + "%")]):
        target = f"/{new}" + row.url_to[len(old) + 1:]
        log.append(("repoint_redirect", row.url_from, row.url_to, target))
        row.write({"url_to": target})

    Page = svc["website.page"].with_context(active_test=False)
    pages = Page.search(
        [(owner_field, "=", owner_id), "|", ("url", "=", f"/{old}"), ("url", "=like", _like_escape(f"/{old}/") + "%")]
    )
    for page in pages:
        rest = page.url[len(old) + 1:]
        target = f"/{new}{rest}"
        clash = Page.search([("url", "=", target), ("website_id", "=", page.website_id.id), ("id", "!=", page.id)], limit=1)
        vals = {"url": target}
        if clash:
            # The page already at the new address wins; this one is kept, unpublished, under a name
            # the owner can find in the dashboard (the editor's page-slug rule: [a-z0-9-], not reserved).
            base = f"/{new}/" + ("former-home" if not rest else rest.strip("/").replace("/", "-") + "-former")
            target, n = base, 2
            while Page.search_count([("url", "=", target), ("website_id", "=", page.website_id.id)]):
                target, n = f"{base}-{n}", n + 1
            vals = {"url": target, "website_published": False}
        log.append(("move_page", page.id, page.url, target, bool(clash)))
        page.write(vals)
        if rest and not clash:
            rewrite.create(
                {
                    "name": f"Redirect {old}{rest} to {new}{rest}",
                    "url_from": f"/{old}{rest}",
                    "url_to": target,
                    "redirect_type": "301",
                    "website_id": False,
                }
            )
    Menu = svc["website.menu"]
    for menu in Menu.search(["|", ("url", "=", f"/{old}"), ("url", "=like", _like_escape(f"/{old}/") + "%")]):
        menu.write({"url": f"/{new}" + menu.url[len(old) + 1:]})
    if log:
        _logger.info("user_websites: re-homed %s %s: /%s -> /%s: %s", owner_field, owner_id, old, new, log)
    return log
