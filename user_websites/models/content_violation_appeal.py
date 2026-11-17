# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
import psycopg2.errors

from odoo import api, models, fields, _
from odoo.exceptions import UserError, ValidationError
from odoo.addons.zero_sudo import public_rpc


# [@ANCHOR: user_websites_content_violation_appeal_content_violation_appeal]
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
        # Moderation history is kept when the user is deleted outright (Bruce, 2026-11-16): the link is
        # cleared and `target_label` still says who it was about.
        ondelete="set null",
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
        ondelete="set null",
        tracking=True,
        default=False,
    )
    target_label = fields.Char(
        string="Appeal About",
        compute="_compute_target_label",
        store=True,
        help="Name of the suspended user or group. Kept when that user or group is deleted, so the appeal stays identifiable.",
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

    @api.model_create_multi
    def create(self, vals_list):
        if self.env.user.share:
            # A portal caller cannot file an appeal that is already decided (F6, ODOO_ACCESS_CONTROL_AUDIT_2026-11-12).
            vals_list = [{k: v for k, v in vals.items() if k != "state"} for vals in vals_list]
        return super().create(vals_list)

    # [@ANCHOR: user_websites:appeal_compute_target_label]
    # Verified by [@ANCHOR: user_websites:test_appeal_survives_user_delete]
    @api.depends("user_id.name", "group_id.name")
    def _compute_target_label(self):
        for appeal in self:
            target = appeal.user_id or appeal.group_id
            # An orphaned appeal (its user or group was deleted) keeps the label it had.
            if target:
                appeal.target_label = target.name
            elif not appeal.target_label:
                appeal.target_label = False

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

    # [@ANCHOR: user_websites:appeal_create_if_none_open]
    # Verified by [@ANCHOR: user_websites:test_appeal_race]
    @api.model
    def _create_if_none_open(self, vals):
        """Create the appeal `vals` (a user's or a group's) unless that target already has one waiting; returns the
        appeal, or an empty recordset when one is waiting.

        The target's row is written (no value changes) before the count. Two simultaneous submissions would otherwise both
        count none: each REPEATABLE READ snapshot predates the other's commit, and a lock would not refresh it. Writing a row
        the other request changed since the snapshot fails with a serialization failure, and Odoo runs the request again."""
        if vals.get("group_id"):
            self.env.cr.execute("UPDATE user_websites_group SET id = id WHERE id = %s", (vals["group_id"],))  # audit-ignore-sql: write-lock on the target's own row
            target = [("group_id", "=", vals["group_id"])]
        else:
            self.env.cr.execute("UPDATE res_users SET id = id WHERE id = %s", (vals["user_id"],))  # audit-ignore-sql: write-lock on the target's own row
            target = [("user_id", "=", vals["user_id"])]
        if self.search_count(target + [("state", "=", "new")]):
            return self.browse()
        return self.create(vals)

    # [@ANCHOR: user_websites:appeal_lock_for_resolution]
    # Verified by [@ANCHOR: user_websites:test_appeal_resolution_refuses_a_row_being_resolved]
    def _lock_for_resolution(self):
        """Row-locks these appeals, then re-reads them, so two moderators cannot both see "new" and
        one approve while the other rejects (the second would overwrite the first and post a false
        note). A row already locked by another moderator is refused at once, not waited for."""
        if not self.ids:
            return
        try:
            with self.env.cr.savepoint():
                self.env.cr.execute(
                    "SELECT id FROM content_violation_appeal WHERE id IN %s FOR UPDATE NOWAIT",
                    (tuple(self.ids),),
                )
        except psycopg2.errors.LockNotAvailable:
            raise UserError(_("Another moderator is resolving this appeal right now. Reload and check its state."))
        self.invalidate_recordset(["state"])

    # [@ANCHOR: user_websites:COMM_appeal_action_approve]
    @public_rpc
    def action_approve(self):
        # Verified by [@ANCHOR: user_websites:test_tour_moderation_appeal]
        """Approves the appeal and pardons the user or group.

        Server-side state guard: the form's own
        `invisible="not id or state != 'new'"` was the only thing preventing
        this from being re-invoked on an already-approved/rejected appeal --
        a direct RPC or a stale concurrent tab could re-run the pardon and
        re-post an "Appeal approved" message with no server-side check.
        """
        self._lock_for_resolution()
        for appeal in self:
            if appeal.state != "new":
                raise UserError(
                    _("This appeal has already been resolved (%s) and cannot be approved again.")
                    % appeal.state
                )
            if not (appeal.user_id or appeal.group_id):
                raise UserError(
                    _("The user or group this appeal was about no longer exists; there is no one to pardon. Reject it to close it.")
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
    @public_rpc
    def action_reject(self):
        """Rejects the appeal.

        Server-side state guard: without it, rejecting an
        already-approved appeal would overwrite `state` back to "rejected" and
        post a "remains suspended" message that is false at that moment
        (the user/group was already pardoned), with no server-side check
        preventing it.
        """
        self._lock_for_resolution()
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
