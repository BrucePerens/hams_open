# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields, _
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

    def _compute_suspended_group_ids(self):
        groups = self.env["user.websites.group"].search(
            [("member_ids", "in", self.ids), ("is_suspended_from_websites", "=", True)],
            limit=1000,
        )

        mapping = {u.id: [] for u in self}
        for g in groups:
            for m in g.member_ids:
                if m.id in mapping:
                    mapping[m.id].append(g.id)

        for user in self:
            user.suspended_group_ids = mapping[user.id]

    def action_suspend_user_websites(self):
        """Forcefully unpublishes all user content and flags them as suspended."""
        user_ids = self.ids

        db_name = self.env.cr.dbname
        # Fire and forget safely without unbounded thread growth
        BACKGROUND_EXECUTOR.submit(_async_unpublish_content, db_name, user_ids)

        for user in self:
            user.is_suspended_from_websites = True

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

    def action_pardon_user_websites(self):
        """Resets strikes and lifts the suspension (Does NOT automatically republish content)."""
        for user in self:
            user.violation_strike_count = 0
            user.is_suspended_from_websites = False
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            user.partner_id.with_user(mail_svc).message_post(
                body=_(
                    "✅ **MODERATION ACTION:** You pardoned this user. The system lifted their suspension and reset their strike count to 0. (Note: Previously unpublished content remains unpublished until manually restored)."
                ),
                subtype_xmlid="mail.mt_note",
            )
