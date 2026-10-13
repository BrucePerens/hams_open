# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fold the pages a renamed site left at its OLD address into the site that holds the slug now.
[@ANCHOR: user_websites:migrate_rehome_stale_site_pages]
Verified by [@ANCHOR: test_migration_rehome_stale_site_pages]

Before 0.7 a slug rename added a 301 from /<old> to /<new> and left the owner's pages at /<old>.
Odoo serves a published page before it reads website.rewrite, so /<old> answered 200 as a second
site, and the owner toolbar's Edit and Unpublish (which address pages through the CURRENT slug)
answered 404. hams_prod had exactly one case: K6BP's home at /bruce-perens-k6bp next to /k6bp.

For each page owned by a member or group whose first path segment is a slug that now redirects (a
301 row whose url_to is the owner's current /<slug>):
  * the page moves to the same path under the current slug; when a page already sits at that
    address it is kept, UNPUBLISHED, at /<slug>/former-home (or /<slug>/<rest>-former), so no
    content is lost and the owner can find it in the dashboard;
  * a moved sub-page gets its own 301 from the old path; the 301 for /<old> itself already exists;
  * website.menu items at the old address move with it.
Nothing is deleted. Every change is logged at INFO with the old and new address and the page's
published flag (the pre-state log); take a pg_dump of website_page, website_menu and website_rewrite
first (docs/stories). Idempotent: a second run finds no page under a retired slug. Plain SQL, no
superuser environment. A redirect chain more than one hop long is left for an operator (logged at
WARNING). The Redis slug/page caches are not reachable from here: flush them after the upgrade.
"""
import logging

_logger = logging.getLogger(__name__)

# (owner column, owner table)
OWNERS = (("owner_user_id", "res_users"), ("user_websites_group_id", "user_websites_group"))


def _free_url(cr, base, website_id):
    url, n = base, 2
    while True:
        cr.execute(
            "SELECT 1 FROM website_page WHERE url = %s AND website_id IS NOT DISTINCT FROM %s", (url, website_id)
        )
        if not cr.fetchone():
            return url
        url, n = f"{base}-{n}", n + 1


def migrate(cr, version):
    moved = []
    for column, table in OWNERS:
        cr.execute(  # audit-ignore-sql: column and table are fixed names from OWNERS above
            f"""
            SELECT p.id, p.url, p.website_id, p.is_published, o.id, o.website_slug
              FROM website_page p
              JOIN {table} o ON o.id = p.{column}
             WHERE o.website_slug IS NOT NULL AND o.website_slug <> ''
               AND p.url ~ '^/[a-z0-9-]+(/.*)?$'
               AND split_part(p.url, '/', 2) <> o.website_slug
            """
        )
        for page_id, url, website_id, published, owner_id, slug in cr.fetchall():
            seg = url.split("/")[1]
            cr.execute(
                "SELECT 1 FROM website_rewrite WHERE url_from = %s AND url_to = %s AND redirect_type = '301' AND active",
                (f"/{seg}", f"/{slug}"),
            )
            if not cr.fetchone():
                _logger.warning(
                    "user_websites: page %s at %s is under %r, which does not redirect to %r; left alone",
                    page_id, url, seg, slug,
                )
                continue
            rest = url[len(seg) + 1:]
            target = f"/{slug}{rest}"
            cr.execute(
                "SELECT 1 FROM website_page WHERE url = %s AND website_id IS NOT DISTINCT FROM %s AND id <> %s",
                (target, website_id, page_id),
            )
            clash = bool(cr.fetchone())
            publish = published
            if clash:
                base = f"/{slug}/" + ("former-home" if not rest else rest.strip("/").replace("/", "-") + "-former")
                target = _free_url(cr, base, website_id)
                publish = False
            cr.execute("UPDATE website_page SET url = %s, is_published = %s WHERE id = %s", (target, publish, page_id))
            cr.execute("UPDATE website_menu SET url = %s WHERE url = %s", (target, url))
            if rest and not clash:
                cr.execute(
                    "INSERT INTO website_rewrite (name, url_from, url_to, redirect_type, active, sequence) "
                    "SELECT %s, %s, %s, '301', true, 0 WHERE NOT EXISTS (SELECT 1 FROM website_rewrite WHERE url_from = %s)",
                    (f"Redirect {url} to {target}", url, target, url),
                )
            moved.append((page_id, url, target, published, publish, clash))
            _logger.info(
                "user_websites: page %s %s -> %s (published %s -> %s, collided: %s)",
                page_id, url, target, published, publish, clash,
            )
    _logger.info("user_websites: re-homed %d pages left under retired site slugs", len(moved))
    return moved
