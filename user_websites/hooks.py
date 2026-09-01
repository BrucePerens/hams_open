# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    # [@ANCHOR: documentation_bootstrap]
    """
    Hook executed upon module installation.
    """
    # Use direct SQL for the initial user group population to circumvent
    # AccessErrors on the restricted 'is_service_account' field.
    user_group = env.ref(
        "user_websites.group_user_websites_user", raise_if_not_found=False
    )
    if user_group:
        public_user = env.ref("base.public_user", raise_if_not_found=False)
        public_user_id = public_user.id if public_user else -1
        with env.cr.savepoint():
            env.cr.execute(
                """
                INSERT INTO res_groups_users_rel (gid, uid)
                SELECT %s, u.id
                FROM res_users u
                WHERE u.id > 0
                  AND u.id != %s
                  AND (u.is_service_account IS NOT TRUE)
                ON CONFLICT DO NOTHING
            """,
                (user_group.id, public_user_id),
            )

    # Backfill `website_page.name` for rows created by a module whose own
    # data files load earlier than user_websites in this install's module
    # graph (e.g. `compliance`'s legal pages). During that earlier module's
    # own data load, `website.page.name` is still stock Odoo's own
    # `_inherits`-delegated field (this module's local `name` field, which
    # shadows that delegation, hasn't been merged into the registry yet),
    # so an explicit `name` given in that XML lands correctly on the linked
    # `ir.ui.view.name` but never reaches this model's own local column --
    # this hook runs after this module's own Python model (and its create()
    # backfill above) are already active, so it can repair those rows using
    # the same "copy from the linked view's name" rule, one time, for
    # whatever already exists in the database at this point in the install.
    # See docs/proposals/COMPLIANCE_PAGE_NAME_LOST_WITH_USER_WEBSITES.md for
    # the full investigation this hook is based on.

    # # Verified by [@ANCHOR: test_website_page_name_backfill_post_init_hook]
    with env.cr.savepoint():
        env.cr.execute(
            """
            UPDATE website_page wp
            SET name = v.name
            FROM ir_ui_view v
            WHERE wp.view_id = v.id
              AND (wp.name IS NULL OR wp.name = '')
              AND v.name IS NOT NULL AND v.name != ''
            """
        )

    with env.cr.savepoint():
        env.cr.execute(
            "CREATE INDEX IF NOT EXISTS idx_website_page_published ON website_page (id) WHERE is_published = TRUE;"
        )
    with env.cr.savepoint():
        env.cr.execute(
            "CREATE INDEX IF NOT EXISTS idx_blog_post_published ON blog_post (id) WHERE is_published = TRUE;"
        )

    # Use direct SQL to update is_service_account as the service account itself cannot see/edit this field
    cf_svc = env.ref("cloudflare.user_cloudflare_purge", raise_if_not_found=False)
    if cf_svc:
        with env.cr.savepoint():
            env.cr.execute(
                "UPDATE res_users SET is_service_account = true WHERE id = %s", (cf_svc.id,)
            )
    uw_svc = env.ref(
        "user_websites.user_websites_service_account", raise_if_not_found=False
    )
    if uw_svc:
        with env.cr.savepoint():
            env.cr.execute(
                "UPDATE res_users SET is_service_account = true WHERE id = %s", (uw_svc.id,)
            )
