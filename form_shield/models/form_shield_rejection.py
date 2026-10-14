# SPDX-License-Identifier: AGPL-3.0-or-later
from datetime import timedelta

from odoo import api, fields, models

SERVICE_ACCOUNT = "form_shield.user_form_shield_service"
DEFAULT_RETENTION_DAYS = 90


class FormShieldRejection(models.Model):
    """One counter per day, form and reason. No address, no email, no text a visitor typed."""

    _name = "form.shield.rejection"
    _description = "Form shield daily counters"
    _order = "day desc, form_id, reason"

    name = fields.Char(compute="_compute_name")
    day = fields.Date(required=True, index=True)
    form_id = fields.Char(string="Form", required=True, index=True)
    reason = fields.Char(required=True, index=True)
    count = fields.Integer(default=0, required=True)

    _day_form_reason_unique = models.Constraint(
        "unique(day, form_id, reason)", "One counter per day, form and reason."
    )

    # [@ANCHOR: form_shield:rejection_name]
    # Verified by [@ANCHOR: form_shield:test_counters_keep_no_personal_data]
    @api.depends("day", "form_id", "reason")
    def _compute_name(self):
        for row in self:
            row.name = "%s %s %s" % (row.day, row.form_id, row.reason)

    # [@ANCHOR: form_shield:count]
    # Verified by [@ANCHOR: form_shield:test_counters_keep_no_personal_data]
    @api.model
    def _count(self, form_id, reason):
        """Adds one to today's counter. Written on its own cursor so a refusal that raises an
        error (and so rolls the request back) is still counted."""
        utils = self.env["zero_sudo.security.utils"]
        service_uid = utils._get_service_uid(SERVICE_ACCOUNT)
        with self.env.registry.cursor() as cursor:
            env = api.Environment(cursor, service_uid, {})
            env["form.shield.rejection"]._upsert(form_id, reason)

    # [@ANCHOR: form_shield:count_upsert]
    # Verified by [@ANCHOR: form_shield:test_counters_keep_no_personal_data]
    @api.model
    def _upsert(self, form_id, reason):
        today = fields.Date.context_today(self)
        self.flush_model()
        # audit-ignore-sql: parameters are passed separately; Tested by [@ANCHOR: form_shield:test_counters_keep_no_personal_data]
        self.env.cr.execute(
            """
            INSERT INTO form_shield_rejection
                   (day, form_id, reason, count, create_date, write_date)
            VALUES (%(day)s, %(form)s, %(reason)s, 1, now(), now())
            ON CONFLICT (day, form_id, reason)
            DO UPDATE SET count = form_shield_rejection.count + 1,
                          write_date = now()
            """,
            {"day": today, "form": form_id[:64], "reason": reason[:64]},
        )
        self.invalidate_model()

    # [@ANCHOR: form_shield:purge]
    # Verified by [@ANCHOR: form_shield:test_purge_drops_old_counters]
    @api.model
    def _cron_purge(self):
        days = self.env["form.shield"]._int_param(
            "form_shield.retention_days", DEFAULT_RETENTION_DAYS
        )
        cutoff = fields.Date.context_today(self) - timedelta(days=days)
        old = self.search([("day", "<", cutoff)], limit=100000)
        old.unlink()
        return len(old)
