# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
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
        env.cr.execute("""
            INSERT INTO res_groups_users_rel (gid, uid)
            SELECT %s, u.id
            FROM res_users u
            WHERE u.id > 0
              AND u.id != %s
              AND (u.is_service_account IS NOT TRUE)
            ON CONFLICT DO NOTHING
        """, (user_group.id, public_user_id))

    env.cr.execute(
        "CREATE INDEX IF NOT EXISTS idx_website_page_published ON website_page (id) WHERE is_published = TRUE;"
    )
    env.cr.execute(
        "CREATE INDEX IF NOT EXISTS idx_blog_post_published ON blog_post (id) WHERE is_published = TRUE;"
    )

    # Use direct SQL to update is_service_account as the service account itself cannot see/edit this field
    cf_svc = env.ref("cloudflare.user_cloudflare_purge", raise_if_not_found=False)
    if cf_svc:
        env.cr.execute("UPDATE res_users SET is_service_account = true WHERE id = %s", (cf_svc.id,))
    uw_svc = env.ref("zero_sudo.user_websites_service_account", raise_if_not_found=False)
    if uw_svc:
         env.cr.execute("UPDATE res_users SET is_service_account = true WHERE id = %s", (uw_svc.id,))
