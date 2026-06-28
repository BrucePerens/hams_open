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

    def action_suspend_user_websites(self):
        """Forcefully unpublishes all user content and flags them as suspended."""
        user_ids = self.ids

        is_test = vars(self.env.registry).get("test_cr") is not None

        if not is_test:
            db_name = self.env.cr.dbname
            # Fire and forget safely without unbounded thread growth
            BACKGROUND_EXECUTOR.submit(_async_unpublish_content, db_name, user_ids)
        else:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            while True:
                pages = (
                    self.env["website.page"]
                    .with_user(svc_uid)
                    .search(
                        [
                            ("owner_user_id", "in", user_ids),
                            "|",
                            ("is_published", "=", True),
                            ("website_published", "=", True),
                        ],
                        limit=5000,
                    )
                )
                if not pages:
                    break
                pages.write({"is_published": False, "website_published": False})
            while True:
                posts = (
                    self.env["blog.post"]
                    .with_user(svc_uid)
                    .search(
                        [
                            ("owner_user_id", "in", user_ids),
                            ("is_published", "=", True),
                        ],
                        limit=5000,
                    )
                )
                if not posts:
                    break
                posts.write({"is_published": False})

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
