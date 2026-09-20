# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import logging
from odoo import models, fields, api

_logger = logging.getLogger(__name__)


class PagerSchedule(models.Model):
    """
    Extends calendar.event to support pager duty scheduling.
    This model is multi-tenant as it inherits from calendar.event, and we add
    website_id to further isolate shifts by website.
    """

    _inherit = "calendar.event"

    is_pager_duty = fields.Boolean(
        string="Is Pager Duty Shift", default=False, index=True
    )
    website_id = fields.Many2one(
        "website", string="Website", ondelete="cascade", index=True
    )

    # The overlapping-shift tiebreak below is Bruce's own decision, answered 2026-09-19 in
    # hams_com/night_shift_questions/answered/
    # pager-schedule-overlapping-on-duty-shifts-winner-rule-a018f28d.md ("Most recent wins.").
    ON_DUTY_SHIFT_ORDER = "create_date desc, id desc"

    @api.model
    def get_current_on_duty_admin(self):
        """
        Returns the user currently on duty for the active website.

        When several `is_pager_duty` shifts are in force at the same moment (nothing in this
        module forbids a double booking, and a deliberate hand-over overlap is a supported
        practice -- see `docs/stories/performance_analytics.md`), the MOST RECENTLY CREATED
        shift wins: `order="create_date desc, id desc"`, with `id desc` breaking a tie between
        two shifts created in the same transaction (PostgreSQL's `now()` is the transaction
        timestamp, so same-transaction rows share a `create_date`). Adding a new shift is
        therefore how an admin overrides an existing one.

        A global shift (`website_id = False`) and a website's own shift are ranked on that one
        axis, with NO precedence for either: the newer of the two wins even when that is the
        global shift. Site-specific-beats-global was a considered and explicitly rejected
        alternative, so that "most recent wins" means the same thing for every pair of
        overlapping shifts and an admin never has to reason about a second rule.
        """
        # [@ANCHOR: test_pager_notification]
        now = fields.Datetime.now()
        domain = [
            ("is_pager_duty", "=", True),
            ("start", "<=", now),
            ("stop", ">=", now),
        ]

        # Enforce strict schema contract. We do not mask missing dependencies.
        if self.env.context.get("website_id"):
            domain += [
                "|",
                ("website_id", "=", False),
                ("website_id", "=", self.env.context.get("website_id")),
            ]
        else:
            current_website = self.env["website"].get_current_website()
            if current_website:
                domain += [
                    "|",
                    ("website_id", "=", False),
                    ("website_id", "=", current_website.id),
                ]

        event = self.env["calendar.event"].search(
            domain, order=self.ON_DUTY_SHIFT_ORDER, limit=1
        )
        if event and event.user_id:
            return event.user_id
        return False
