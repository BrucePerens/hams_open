# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Move every member and group site's home page from /<slug>/home to /<slug>.
[@ANCHOR: user_websites:migrate_home_url_to_slug]
Verified by [@ANCHOR: test_migration_home_url_to_slug]

Bruce, 2026-10-12: "make all of the user websites be hams.com/<callsign> without 'home'."
A member site's home is now served at /<slug> by controllers/main.py's user_home_fallback(),
and /<slug>/home answers a permanent 301 to /<slug> (user_home_legacy_redirect()), so existing
links, bookmarks and search-engine entries keep working without a stored redirect row.

Plain SQL, like every other migration in this codebase: the zero-sudo rule forbids building a
superuser environment here. Only website.page rows that belong to a member or a group (owner or
group set) and whose url is exactly '/<slug>/home' are touched, and a row is left alone when the
target '/<slug>' already exists on the same website (the unique(url, website_id) constraint would
refuse the rename, and the page already there wins). website.menu items that pointed at the old
address are moved with them. Idempotent: a second run finds nothing to move.
"""
import logging

_logger = logging.getLogger(__name__)


def migrate(cr, version):
    cr.execute(  # audit-ignore-sql: static SQL, no parameters
        """
        UPDATE website_page p
           SET url = regexp_replace(p.url, '/home$', '')
         WHERE p.url ~ '^/[a-z0-9-]+/home$'
           AND (p.owner_user_id IS NOT NULL OR p.user_websites_group_id IS NOT NULL)
           AND NOT EXISTS (
                SELECT 1 FROM website_page q
                 WHERE q.url = regexp_replace(p.url, '/home$', '')
                   AND q.website_id IS NOT DISTINCT FROM p.website_id
           )
        RETURNING p.url
        """
    )
    moved = [row[0] for row in cr.fetchall()]
    for url in moved:
        cr.execute(  # audit-ignore-sql: parameterised
            "UPDATE website_menu SET url = %s WHERE url = %s",
            (url, url + "/home"),
        )
    _logger.info("user_websites: moved %d member-site home pages to /<slug>", len(moved))
