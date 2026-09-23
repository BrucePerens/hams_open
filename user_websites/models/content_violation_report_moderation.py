# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
user_websites' own extension of the generic `content.violation.report`
model (content_moderation module, extracted from this module 2026-09-23).

Owns everything about that report/strike/suspend pipeline that is
genuinely specific to personal/group *websites*: the `content_group_id`
target field (hard-typed to `user.websites.group`, which content_moderation
has never heard of), the `_increment_strike_count()` atomic-lock helper
(it calls a Postgres procedure this module's own db_functions.init()
creates -- see sql_views.py -- and that procedure only knows how to
increment `violation_strike_count` on `res_users`/`user_websites_group`,
both of which are fields THIS module adds, not core Odoo or
content_moderation), the `_apply_enforcement_action()` override that
actually strikes-and-suspends, and the "notify admins of a pending-report
backlog" cron/mail-template pairing (kept here rather than genericized:
its own company-scoped abuse-email config parameter
(`user_websites.company_abuse_email`) and its service account
(`user_websites.user_websites_service_account`) are both this module's own
naming, and there is no second consumer wired up yet to justify inventing
a cross-module config-parameter/service-account convention for a "notify
my domain's moderators of a backlog" feature -- see this extraction's own
report for the full reasoning, and content.violation.report's own
`_apply_enforcement_action()` docstring in content_moderation for the
matching reasoning on the enforcement side).
"""
from odoo import models, fields, api, _
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation


class ContentViolationReportModeration(models.Model):
    _inherit = "content.violation.report"

    content_group_id = fields.Many2one(
        "user.websites.group",
        string="Content Group",
        ondelete="set null",
        tracking=True,
    )

    @api.model
    def _cron_notify_pending_reports(self):
        # [@ANCHOR: user_websites:cron_notify_pending_reports]

        # Verified by [@ANCHOR: user_websites:test_cron_pending_reports]

        # Verified by [@ANCHOR: user_websites:COMM_test_cron_pending_reports]
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
        # content_violation_report_moderator_rule's own unconditionally-
        # permissive domain (content_moderation's own rule, formerly this
        # module's own content_violation_report_admin_rule) to see across
        # companies at all, which is a separate, already-flagged
        # over-permissive rule, not a safe foundation for this fix.
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
                    template.with_user(mail_svc).with_company(company).with_context(pending_count=count).send_mail(company.id, force_send=False, email_values=email_vals)  # audit-ignore-mail: Tested by [@ANCHOR: user_websites:test_cron_pending_reports]  # fmt: skip

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

    # [@ANCHOR: user_websites:COMM_apply_enforcement_action]
    def _apply_enforcement_action(self):
        """Override of content_moderation's generic extension hook: applies
        this module's own real consequence for an upheld report -- a strike
        against the reported content's owner (a res.users) or group (a
        user.websites.group), and past 3 strikes, suspension. Moved here
        verbatim (same logic, same stored-procedure/locking/messaging
        behavior) from this module's own former
        ContentViolationReport.action_take_action_and_strike(), whose state
        guard and no-op-on-terminal-state behavior now live in
        content_moderation's own action_take_action_and_strike() (the base
        module's action_take_action_and_strike() already applied the
        report.state = "action_taken" transition and the terminal-state
        skip before calling this hook -- this override's only job is the
        consequence itself). The original test-verification anchor link for
        this behavior stayed on content_moderation's own
        action_take_action_and_strike(), not duplicated here.
        """
        self.ensure_one()
        if self.content_owner_id:
            # The caller (Moderator) already has explicit write access to res.users
            owner = self.content_owner_id

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
            self.with_user(mail_svc).message_post(
                body=_(
                    "You applied a strike to the owner. Current strike count: %s"
                )
                % owner.violation_strike_count,
                subtype_xmlid="mail.mt_note",
            )
        elif self.content_group_id:
            group = self.content_group_id

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
            self.with_user(mail_svc).message_post(
                body=_(
                    "You applied a strike to the group. Current strike count: %s"
                )
                % group.violation_strike_count,
                subtype_xmlid="mail.mt_note",
            )
        else:
            # Neither an owner nor a group was resolved for this report
            # (e.g. the reported URL didn't match any known slug) -- fall
            # back to content_moderation's own generic behavior (an
            # audit-trail note, no consequence) rather than silently doing
            # nothing with no chatter trail at all.
            super()._apply_enforcement_action()
