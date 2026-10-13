# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Report any member or group site whose slug is now a reserved word. Changes no data.
[@ANCHOR: user_websites:migrate_report_reserved_slug_collisions]
Verified by [@ANCHOR: test_migration_report_reserved_slug_collisions]

The reserved list grew in 0.6 (every first segment of a platform route is reserved, and the list
is also derived from the live routing map). A slug claimed before that and equal to such a word is
a site nobody can reach, because the static route wins over /<slug>. This step only reads: it
names each collision in the log at ERROR level and returns them, so an operator can rename the
site deliberately (renaming moves a member's address, which is not a decision for a migration).
It checks the fixed list only: the routing map is not built while a migration runs, so the
routing-map half of the check is the model constraint (edge_routing.utils.reserved_slug_words) and
a read-only look at a database before a deploy. Plain SQL, no superuser environment.
"""
import logging

from odoo.addons.edge_routing.utils import RESERVED_SLUGS

_logger = logging.getLogger(__name__)


def migrate(cr, version):
    collisions = []
    for kind, table in (("member", "res_users"), ("group", "user_websites_group")):
        cr.execute(  # audit-ignore-sql: table name is one of two literals above
            "SELECT id, website_slug FROM " + table + " WHERE website_slug IS NOT NULL AND website_slug <> ''"
        )
        collisions += [(kind, row_id, slug) for row_id, slug in cr.fetchall() if slug.lower() in RESERVED_SLUGS]
    for kind, row_id, slug in collisions:
        _logger.error("user_websites: %s id %s has the reserved slug %r; its site is unreachable until renamed", kind, row_id, slug)
    _logger.info("user_websites: %d reserved-slug collisions found", len(collisions))
    return collisions
