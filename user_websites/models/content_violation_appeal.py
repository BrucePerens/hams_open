# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import api, models, fields, _
from odoo.exceptions import ValidationError


class ContentViolationAppeal(models.Model):
    _name = "content.violation.appeal"
    _description = "User Website Moderation Appeal"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _inherit = ["mail.thread", "mail.activity.mixin"]
    _order = "create_date desc"

    user_id = fields.Many2one(
        "res.users",
        string="Suspended User",
        required=False,
        ondelete="cascade",
        tracking=True,
    )
    group_id = fields.Many2one(
        "user.websites.group",
        string="Suspended Group",
        required=False,
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

    @api.constrains("user_id", "group_id")
    def _check_appeal_target(self):
        for appeal in self:
            if bool(appeal.user_id) == bool(appeal.group_id):
                raise ValidationError(
                    _(
                        "An appeal must be tied to either a User or a Group, but not both."
                    )
                )

    def action_approve(self):
        # # Tested by [@ANCHOR: user_websites:test_tour_moderation_appeal]
        """Approves the appeal and pardons the user or group."""
        # ADR 0078: Fetch service account outside the loop for O(1) Memory Mapping
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        for appeal in self:
            appeal.state = "approved"
            if appeal.group_id:
                appeal.group_id.action_pardon_group_websites()
                message = _(
                    "Appeal approved. You pardoned the group and lifted their suspension."
                )
            else:
                appeal.user_id.action_pardon_user_websites()
                message = _(
                    "Appeal approved. You pardoned the user and lifted their suspension."
                )
            appeal.with_user(mail_svc).message_post(
                body=message,
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
            message = (
                _("Appeal rejected. The group remains suspended.")
                if appeal.group_id
                else _("Appeal rejected. The user remains suspended.")
            )
            appeal.with_user(mail_svc).message_post(
                body=message,
                subtype_xmlid="mail.mt_note",
            )
