# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation
from odoo.exceptions import UserError


class ContentViolationReport(models.Model):
    _name = "content.violation.report"
    _description = "User Website Content Violation Report"
    name = fields.Char(string="Name", default=lambda self: self._description)
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

    _report_uniq = models.Constraint("UNIQUE(target_url, reported_by_user_id)", "You have already submitted a report for this URL.")
    _url_not_empty = models.Constraint("CHECK(LENGTH(TRIM(target_url)) > 0)", "The target URL cannot be empty.")
    _desc_not_empty = models.Constraint("CHECK(LENGTH(TRIM(description)) > 0)", "The description cannot be empty.")

    @api.model
    def _cron_notify_pending_reports(self):
        # [@ANCHOR: cron_notify_pending_reports]

        # Verified by [@ANCHOR: test_cron_pending_reports]

        # Verified by [@ANCHOR: COMM_test_cron_pending_reports]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        # Bug-hunt fix (bug class 17, 2026-09-09): this used to iterate
        # res.company.search([], limit=10000) -- every company in the whole
        # database, not just the ones this service account is actually
        # scoped to. That reached toward (and crashed on, via an uncaught
        # AccessError) every company outside the account's own company_ids
        # (data/user_websites_data.xml scopes it to base.main_company only;
        # this module doesn't dynamically provision per-tenant companies of
        # its own today, so that's genuinely the account's real, intended
        # scope, not an under-provisioning bug -- see ADR 0083). Iterating
        # the account's own company_ids instead means an out-of-scope
        # company is never in the loop to begin with, matching this claim's
        # own corrected root-cause fix (see
        # hams_com/docs/bug_hunt_interim_claims/hams_open/user_websites/
        # claims/cron_notify_pending_reports.md): if this cron is ever meant
        # to serve additional companies, that's a real provisioning change
        # (grant the service account access to those companies), not a
        # widening of what this loop iterates blindly.
        #
        # NOT converted to a single grouped _read_group() call outside this
        # loop: .with_company(company) per iteration is what makes each
        # company's own records visible at all under this module's own
        # ir.rule -- a single ungrouped call would depend on
        # content_violation_report_admin_rule's own unconditionally-
        # permissive domain to see across companies at all, which is a
        # separate, already-flagged over-permissive rule, not a safe
        # foundation for this fix.
        for company in self.env["res.users"].browse(svc_uid).company_ids:
            count = self.with_user(svc_uid).with_company(company).search_count([("state", "=", "new")])  # burn-ignore-company-scoped-loop

            if count > 0:
                template = self.env.ref(
                    "user_websites.email_template_pending_violations_summary",
                    raise_if_not_found=False,
                )
                if template:
                    abuse_email = self.env["zero_sudo.security.utils"].with_company(company)._get_system_param(
                        "user_websites.company_abuse_email"
                    )
                    if not abuse_email:
                        abuse_email = company.email or "admin@example.com"

                    email_vals = {"email_to": abuse_email}
                    mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                        "zero_sudo.mail_service_internal"
                    )
                    template.with_user(mail_svc).with_company(company).with_context(pending_count=count).send_mail(company.id, force_send=False, email_values=email_vals)  # audit-ignore-mail: Tested by [@ANCHOR: test_cron_pending_reports]  # fmt: skip

    # [@ANCHOR: user_websites:COMM_increment_strike_count]
    def _increment_strike_count(self, table_name, rec_id):
        """Atomically locks and increments a strike count via the
        increment_strike_count() stored procedure (sql_views.py).

        Pulled out as its own method (rather than an inline cr.execute
        call) so tests can safely wrap/verify it without mocking the
        database cursor itself, which is forbidden.
        """
        self.env.cr.execute(
            "SELECT increment_strike_count(%s, %s)", (table_name, rec_id)
        )

    # --- Moderation Action Methods ---
    #
    # Server-side state guards (bug class 18): the client-rendered form's own
    # `invisible="state != 'new'"`-style attributes were the ONLY thing
    # preventing these from being invoked on an already-resolved report --
    # a direct RPC, a stale concurrent admin tab, or (for
    # action_take_action_and_strike specifically) an automated caller could
    # re-process a report a second time with no server-side check at all.

    # [@ANCHOR: user_websites:COMM_action_mark_under_review]
    def action_mark_under_review(self):
        for report in self:
            if report.state in ("action_taken", "dismissed"):
                raise UserError(
                    _(
                        "This report has already been resolved (%s) and cannot be reopened for review."
                    )
                    % report.state
                )
        self.write({"state": "under_review"})

    # [@ANCHOR: user_websites:COMM_report_action_dismiss]
    def action_dismiss(self):
        for report in self:
            if report.state in ("action_taken", "dismissed"):
                raise UserError(
                    _(
                        "This report has already been resolved (%s) and cannot be dismissed again."
                    )
                    % report.state
                )
        self.write({"state": "dismissed"})

    def action_take_action_and_strike(self):
        # [@ANCHOR: action_take_action_and_strike]

        # Verified by [@ANCHOR: test_moderation_suspension]
        """
        Marks the report as validated, sets state to 'action_taken',
        and increments the owner's strike count. Enforces the 3-strike rule.

        No-ops (does not re-strike) for a report already in a terminal state
        (action_taken/dismissed): the real caller in website_page.py's
        automated SSTI/XSS-strip hook looks up an existing report by
        (target_url, reported_by_user_id) only, and the unique constraint on
        that pair means a second detection for the same URL/reporter reuses
        the SAME report row -- without this guard, re-triggering the
        sanitizer (e.g. saving the same page again) re-struck and
        re-suspended an account that had already been fully processed.
        """
        for report in self:
            if report.state in ("action_taken", "dismissed"):
                continue
            report.state = "action_taken"

            if report.content_owner_id:
                # The caller (Admin) already has explicit write access to res.users
                owner = report.content_owner_id

                # Use stored procedure to atomically lock and increment against the raw DB state
                self._increment_strike_count("res_users", owner.id)
                notify_model_invalidation(self.env, "res.users")
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

                if group:
                    # Use stored procedure to atomically lock and increment
                    self._increment_strike_count("user_websites_group", group.id)
                    notify_model_invalidation(self.env, "user.websites.group")
                    group.invalidate_recordset(["violation_strike_count"])

                    if (
                        group.violation_strike_count >= 3
                        and not group.is_suspended_from_websites
                    ):
                        group.action_suspend_group_websites()

                mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "zero_sudo.mail_service_internal"
                )
                report.with_user(mail_svc).message_post(
                    body=_(
                        "You applied a strike to the group. Current strike count: %s"
                    )
                    % group.violation_strike_count,
                    subtype_xmlid="mail.mt_note",
                )
