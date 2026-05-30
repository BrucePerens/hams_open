# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, fields, _


class ContentViolationAppeal(models.Model):
    _name = "content.violation.appeal"
    _description = "User Website Moderation Appeal"
    _inherit = ["mail.thread", "mail.activity.mixin"]
    _order = "create_date desc"

    user_id = fields.Many2one(
        "res.users",
        string="Suspended User",
        required=True,
        ondelete="cascade",
        tracking=True,
    )
    reason = fields.Text(string="Appeal Reason", required=True)

    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
    )

    state = fields.Selection(
        [
            ("new", "Pending Review"),
            ("approved", "Approved (Pardoned)"),
            ("rejected", "Rejected"),
        ],
        string="Status",
        default="new",
        tracking=True,
        index=True,
    )

    def action_approve(self):
        # Tested by [@ANCHOR: user_websites:test_tour_moderation_appeal]
        """Approves the appeal and pardons the user."""
        # ADR 0078: Fetch service account outside the loop for O(1) Memory Mapping
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        for appeal in self:
            appeal.state = "approved"
            appeal.user_id.action_pardon_user_websites()
            appeal.with_user(mail_svc).message_post(
                body=_(
                    "Appeal approved. You pardoned the user and lifted their suspension."
                ),
                subtype_xmlid="mail.mt_note",
            )

    def action_reject(self):
        """Rejects the appeal."""
        # ADR 0078: Fetch service account outside the loop for O(1) Memory Mapping
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        for appeal in self:
            appeal.state = "rejected"
            appeal.with_user(mail_svc).message_post(
                body=_("Appeal rejected. The user remains suspended."),
                subtype_xmlid="mail.mt_note",
            )
