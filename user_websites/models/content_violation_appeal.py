# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
from odoo import api, models, fields, _
from odoo.exceptions import UserError, ValidationError


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
        # Odoo only runs @api.constrains methods whose watched fields were
        # actually present in the create()/write() vals; an explicit default
        # (even False) guarantees user_id/group_id are always in vals, so
        # _check_appeal_target() reliably fires even when a caller omits
        # both fields, rather than silently allowing an appeal tied to
        # neither a user nor a group.
        default=False,
    )
    group_id = fields.Many2one(
        "user.websites.group",
        string="Suspended Group",
        required=False,
        ondelete="cascade",
        tracking=True,
        default=False,
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
    # [@ANCHOR: user_websites:COMM_check_appeal_target]
    def _check_appeal_target(self):
        for appeal in self:
            if bool(appeal.user_id) == bool(appeal.group_id):
                raise ValidationError(
                    _(
                        "An appeal must be tied to either a User or a Group, but not both."
                    )
                )

    # [@ANCHOR: user_websites:COMM_appeal_action_approve]
    def action_approve(self):
        # Verified by [@ANCHOR: user_websites:test_tour_moderation_appeal]
        """Approves the appeal and pardons the user or group.

        Server-side state guard (bug class 18): the form's own
        `invisible="not id or state != 'new'"` was the only thing preventing
        this from being re-invoked on an already-approved/rejected appeal --
        a direct RPC or a stale concurrent tab could re-run the pardon and
        re-post an "Appeal approved" message with no server-side check.
        """
        for appeal in self:
            if appeal.state != "new":
                raise UserError(
                    _("This appeal has already been resolved (%s) and cannot be approved again.")
                    % appeal.state
                )
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

    # [@ANCHOR: user_websites:COMM_appeal_action_reject]
    def action_reject(self):
        """Rejects the appeal.

        Server-side state guard (bug class 18): without this, rejecting an
        already-approved appeal overwrote `state` back to "rejected" and
        posted a "remains suspended" message that was false at that moment
        (the user/group was already pardoned), with no server-side check
        preventing it.
        """
        for appeal in self:
            if appeal.state != "new":
                raise UserError(
                    _("This appeal has already been resolved (%s) and cannot be rejected again.")
                    % appeal.state
                )
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
