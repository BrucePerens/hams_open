# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields, _
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation
from .res_users import _async_unpublish_content, BACKGROUND_EXECUTOR


class ResUsersModeration(models.Model):
    """
    Feature-specific extension of res.users to handle the
    Three-Strikes moderation, suspension logic, and high-performance slug caching.
    """

    _inherit = "res.users"

    violation_strike_count = fields.Integer(
        string="Violation Strikes",
        default=0,
        help="Number of upheld content violations. Hitting 3 triggers an automatic suspension.",
    )
    is_suspended_from_websites = fields.Boolean(
        string="Suspended from Websites",
        default=False,
        help="If True, all personal pages and blogs are forcefully unpublished and locked.",
    )

    suspended_group_ids = fields.Many2many(
        "user.websites.group",
        compute="_compute_suspended_group_ids",
        string="Suspended Groups",
    )

    # [@ANCHOR: user_websites:COMM_compute_suspended_group_ids]
    def _compute_suspended_group_ids(self):
        # Paginate instead of a single capped search(limit=1000): a flat cap
        # with no continuation silently dropped any suspended groups beyond
        # the first 1000 matching (member_ids in self.ids AND
        # is_suspended_from_websites=True) -- for a large batch compute (e.g.
        # rendering an admin list view over many users) that under-reports
        # `suspended_group_ids` for some users with no signal at all. Mirrors
        # the keyset-pagination idiom already used elsewhere in this module
        # (res_users.py's GDPR export/purge loops) for "must see every
        # matching row, not just the first batch" reads.
        groups = self.env["user.websites.group"]
        last_id = 0
        while True:
            batch = self.env["user.websites.group"].search(
                [
                    ("id", ">", last_id),
                    ("member_ids", "in", self.ids),
                    ("is_suspended_from_websites", "=", True),
                ],
                limit=1000,
                order="id asc",
            )
            if not batch:
                break
            groups |= batch
            last_id = batch[-1].id
            if len(batch) < 1000:
                break

        mapping: dict[int, list[int]] = {u.id: [] for u in self}
        for g in groups:
            for m in g.member_ids:
                if m.id in mapping:
                    mapping[m.id].append(g.id)

        for user in self:
            user.suspended_group_ids = mapping[user.id]

    # [@ANCHOR: user_websites:COMM_action_suspend_user_websites]
    def action_suspend_user_websites(self):
        """Forcefully unpublishes all user content and flags them as suspended."""
        user_ids = self.ids

        db_name = self.env.cr.dbname
        # Defer the submit to a postcommit callback (matches res_users.py's
        # write()-triggered archival path for this exact same async
        # function) rather than firing it immediately: this method's own
        # for-loop below (setting is_suspended_from_websites and posting the
        # audit message) can still raise -- e.g. message_post() failing for
        # one user in a multi-user batch -- which rolls back this whole
        # transaction, including every is_suspended_from_websites write and
        # audit message. Firing the background unpublish immediately, before
        # that outcome is known, let a rolled-back suspension still leave
        # user content silently unpublished with no suspension flag and no
        # audit trail to explain why. cr.postcommit only runs if this
        # transaction's own commit() actually happens (Cursor.commit() calls
        # postcommit.run() right after committing; rollback() clears the
        # queued callbacks instead) -- see odoo/sql_db.py.
        self.env.cr.postcommit.add(
            lambda: BACKGROUND_EXECUTOR.submit(
                _async_unpublish_content, db_name, user_ids
            )
        )

        for user in self:
            user.is_suspended_from_websites = True

            # _get_page_id_by_url() (website_page.py) caches resolved page
            # ids keyed by URL and checks owner_user_id.is_suspended_from_websites
            # at cache-population time -- without this, a page resolved and
            # cached before suspension would keep resolving after suspension
            # until the distributed cache entry happened to expire on its own.
            notify_model_invalidation(self.env, "website.page")

            # Note: We use Odoo's mail.thread on the underlying partner to log the suspension
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            user.partner_id.with_user(mail_svc).message_post(
                body=_(
                    "🚨 **AUTOMATED ACTION:** The system suspended this user for accumulating 3 or more violation strikes and unpublished their personal content."
                ),
                subtype_xmlid="mail.mt_note",
            )

    # [@ANCHOR: user_websites:COMM_action_pardon_user_websites]
    def action_pardon_user_websites(self):
        """Resets strikes and lifts the suspension (Does NOT automatically republish content)."""
        for user in self:
            user.violation_strike_count = 0
            user.is_suspended_from_websites = False
            notify_model_invalidation(self.env, "website.page")
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            user.partner_id.with_user(mail_svc).message_post(
                body=_(
                    "✅ **MODERATION ACTION:** You pardoned this user. The system lifted their suspension and reset their strike count to 0. (Note: Previously unpublished content remains unpublished until manually restored)."
                ),
                subtype_xmlid="mail.mt_note",
            )
