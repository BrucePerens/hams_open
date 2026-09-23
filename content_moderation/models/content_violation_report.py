# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.exceptions import UserError


class ContentViolationReport(models.Model):
    """Content-type-agnostic report against a reachable piece of content.

    Extraction note (2026-09-23): this model used to live entirely inside
    `user_websites`, keyed by `target_url` and a `content_group_id` hard-typed
    to `user.websites.group`. It was already generic at its core -- nothing
    here actually required a personal/group website -- so the extraction
    moved the model itself here unchanged (same `_name`, so no data
    migration: the underlying `content_violation_report` table doesn't care
    which addon's `ir.model.data` row claims to own the model) and left
    `content_group_id` behind for `user_websites` to add back via its own
    `_inherit = "content.violation.report"` (models/
    content_violation_report_moderation.py), the same way any other
    module-specific extension field would be added to a shared model.
    `content_owner_id` (a plain `res.users` reference) stayed here: unlike
    `content_group_id`, it makes no assumption about violation_strike_count
    or any other user_websites-only field -- it is genuinely just "who to
    blame for this content," which every consumer this module was extracted
    for (forum posts, classifieds, private messages, QSO recordings, and
    user_websites' own pages) can resolve the same way.
    """

    _name = "content.violation.report"
    _description = "Content Violation Report"
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

    # Note: Target owner is resolved by the reporting caller (e.g. a
    # controller) during submission -- this base module has no opinion on
    # how a URL/target maps to an owner.
    content_owner_id = fields.Many2one(
        "res.users", string="Content Owner", ondelete="set null", tracking=True
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

    # --- Moderation Action Methods ---
    #
    # Server-side state guards (bug class 18, found in user_websites before
    # this extraction): the client-rendered form's own
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

        # Verified by [@ANCHOR: user_websites:test_moderation_suspension]
        """
        Marks the report as validated (state='action_taken'), then delegates
        the actual real-world consequence to _apply_enforcement_action() --
        see that method's own docstring for why this base module cannot
        implement a real consequence itself.

        No-ops (does not re-strike) for a report already in a terminal state
        (action_taken/dismissed): user_websites' own automated SSTI/XSS-strip
        hook (website_page.py) looks up an existing report by (target_url,
        reported_by_user_id) only, and the unique constraint on that pair
        means a second detection for the same URL/reporter reuses the SAME
        report row -- without this guard, re-triggering a sanitizer (e.g.
        saving the same page again) would re-apply the enforcement
        consequence a second time on an already-fully-processed report. This
        guard is deliberately kept here, in the base module, rather than
        pushed into each override, since "don't re-process a resolved
        report" is a property of the report/state machine itself, not of any
        one consequence.
        """
        for report in self:
            if report.state in ("action_taken", "dismissed"):
                continue
            report.state = "action_taken"
            report._apply_enforcement_action()

    def _apply_enforcement_action(self):
        # [@ANCHOR: apply_enforcement_action]
        """Extension hook: override this (via `_inherit =
        "content.violation.report"`) to apply your own module's real
        consequence for an upheld report against ITS kind of content.
        Called once per report, after `state` has already been set to
        'action_taken', from `action_take_action_and_strike()` above.

        `user_websites`' own override (content_violation_report_moderation.py)
        is the reference implementation: it strikes the report's
        `content_owner_id` (or its own `content_group_id` extension field)
        via the same atomic stored-procedure pattern, and past 3 strikes,
        suspends and unpublishes that user's or group's website content.

        This base module's own default is deliberately a no-op beyond an
        audit-trail note, NOT "strike content_owner_id via the generic
        stored procedure, just skip the suspension" (an earlier draft of
        this extraction's own design considered exactly that as the
        "obvious" generic default). Reading the actual code it would call
        ruled that out: `increment_strike_count()` is a Postgres procedure
        created by `user_websites.db_functions.init()`
        (user_websites/models/sql_views.py), and the `violation_strike_count`
        column it writes only exists on `res_users`/`user_websites_group`
        because `user_websites` (res_users_moderation.py /
        user_websites_groups.py) adds it there. In a hypothetical install of
        this module WITHOUT user_websites (or any other consumer) present,
        that "generic" default would hard-crash the moment a moderator
        clicked "Take Action & Strike" -- worse than doing nothing, since it
        breaks the one action this model exists to support. A real second
        consumer (e.g. a future forum/classifieds moderation module) is
        expected to add its own override exactly like user_websites does,
        wiring whatever consequence (mute, delist, ban) actually makes sense
        for its own content type and its own strike/suspension fields.
        """
        self.ensure_one()
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        self.with_user(mail_svc).message_post(
            body=_(
                "Report marked as action taken. No enforcement handler is "
                "configured for this report's target in the installed "
                "modules, so no automatic consequence was applied."
            ),
            subtype_xmlid="mail.mt_note",
        )
