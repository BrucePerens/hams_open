# -*- coding: utf-8 -*-
from odoo import models, fields, api, _


class ContentViolationReport(models.Model):
    _name = "content.violation.report"
    _description = "User Website Content Violation Report"
    _inherit = ["mail.thread", "mail.activity.mixin"]
    _order = "create_date desc"

    target_url = fields.Char(
        string="Reported URL", required=True, tracking=True, index=True
    )
    description = fields.Text(string="Violation Description", required=True)

    state = fields.Selection(
        [
            ("new", "New"),
            ("under_review", "Under Review"),
            ("action_taken", "Action Taken (Strike)"),
            ("dismissed", "Dismissed"),
        ],
        string="Status",
        default="new",
        tracking=True,
        index=True,
    )

    # Note: Target owner is resolved by the controller during submission
    content_owner_id = fields.Many2one(
        "res.users", string="Content Owner", ondelete="set null", tracking=True
    )
    content_group_id = fields.Many2one(
        "user.websites.group",
        string="Content Group",
        ondelete="set null",
        tracking=True,
    )

    reported_by_user_id = fields.Many2one(
        "res.users", string="Reported By (Internal User)", ondelete="set null"
    )
    reported_by_email = fields.Char(string="Reported By (Guest Email)")

    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
    )

    _report_uniq = models.Constraint(
        "UNIQUE(target_url, reported_by_user_id)",
        "You have already submitted a report for this URL.",
    )
    _url_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(target_url)) > 0)", "The target URL cannot be empty."
    )
    _desc_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(description)) > 0)", "The description cannot be empty."
    )

    @api.model
    def _cron_notify_pending_reports(self):
        # [@ANCHOR: cron_notify_pending_reports]
        # Verified by [@ANCHOR: test_cron_pending_reports]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        count = self.with_user(svc_uid).search_count([("state", "=", "new")])

        if count > 0:
            template = self.env.ref(
                "user_websites.email_template_pending_violations_summary",
                raise_if_not_found=False,
            )
            if template:
                abuse_email = self.env["zero_sudo.security.utils"]._get_system_param(
                    "user_websites.company_abuse_email"
                )
                if not abuse_email:
                    abuse_email = self.env.company.email or "admin@example.com"

                email_vals = {"email_to": abuse_email}
                mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "zero_sudo.mail_service_internal"
                )
                template.with_user(mail_svc).with_context(pending_count=count).send_mail(self.env.company.id, force_send=False, email_values=email_vals)  # audit-ignore-mail: Tested by [@ANCHOR: test_cron_pending_reports]  # fmt: skip

    # --- Moderation Action Methods ---
    def action_mark_under_review(self):
        self.write({"state": "under_review"})

    def action_dismiss(self):
        self.write({"state": "dismissed"})

    def action_take_action_and_strike(self):
        # [@ANCHOR: action_take_action_and_strike]
        # Verified by [@ANCHOR: test_moderation_suspension]
        """
        Marks the report as validated, sets state to 'action_taken',
        and increments the owner's strike count. Enforces the 3-strike rule.
        """
        for report in self:
            report.state = "action_taken"

            if report.content_owner_id:
                # The caller (Admin) already has explicit write access to res.users
                owner = report.content_owner_id

                # RACE CONDITION FIX: Row-level lock to prevent 'Lost Update' on concurrent strikes
                self.env.cr.execute(
                    "SELECT id FROM res_users WHERE id = %s FOR NO KEY UPDATE",
                    (owner.id,),
                )

                # Bypass ORM to ensure atomic increment against the raw DB state
                self.env.cr.execute(
                    "UPDATE res_users SET violation_strike_count = violation_strike_count + 1 WHERE id = %s",
                    (owner.id,),
                )
                owner.invalidate_recordset(["violation_strike_count"])

                # Enforce the 3-Strike Rule
                if (
                    owner.violation_strike_count >= 3
                    and not owner.is_suspended_from_websites
                ):
                    owner.action_suspend_user_websites()

                mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "zero_sudo.mail_service_internal"
                )
                report.with_user(mail_svc).message_post(
                    body=_(
                        "You applied a strike to the owner. Current strike count: %s"
                    )
                    % owner.violation_strike_count,
                    subtype_xmlid="mail.mt_note",
                )
            elif report.content_group_id:
                group = report.content_group_id

                if group.member_ids:
                    # RACE CONDITION FIX: Lock all group members to prevent 'Lost Update' on concurrent strikes
                    self.env.cr.execute(
                        "SELECT id FROM res_users WHERE id IN %s FOR NO KEY UPDATE",
                        (tuple(group.member_ids.ids),),
                    )

                    self.env.cr.execute(
                        "UPDATE res_users SET violation_strike_count = violation_strike_count + 1 WHERE id IN %s",
                        (tuple(group.member_ids.ids),),
                    )
                    group.member_ids.invalidate_recordset(["violation_strike_count"])

                    for member in group.member_ids:
                        if (
                            member.violation_strike_count >= 3
                            and not member.is_suspended_from_websites
                        ):
                            member.action_suspend_user_websites()
                mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "zero_sudo.mail_service_internal"
                )
                report.with_user(mail_svc).message_post(
                    body=_("You applied a strike to all members of the group."),
                    subtype_xmlid="mail.mt_note",
                )
