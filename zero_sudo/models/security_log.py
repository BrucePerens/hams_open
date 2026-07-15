# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, fields, api
import datetime


class SecurityLog(models.Model):
    # [@ANCHOR: zero_sudo:COMM_zero_sudo_security_log_global]
    # ---
    # # Verified by [@ANCHOR: zero_sudo:COMM_test_security_log_immutability]
    # ---
    # This model is logically GLOBAL and NOT multi-tenanted.
    # It records blocked service account login attempts for auditing.
    _name = "zero_sudo.security.log"
    _description = "Zero-Sudo Security Audit Log"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _order = "create_date desc"

    user_id = fields.Many2one(
        "res.users", string="Target User", required=False, index=True
    )
    login = fields.Char(string="Login Used", index=True)
    ip_address = fields.Char(string="IP Address", index=True)
    user_agent = fields.Char(string="User Agent")
    reason = fields.Selection(
        [
            ("service_account_blocked", "Service Account Web Login Attempt"),
            ("privilege_escalation_trip", "God-Mode Security Block Tripped"),
            ("param_access_denied", "Unauthorized System Parameter Access"),
            ("param_write_denied", "Unauthorized System Parameter Write"),
            ("cache_invalidation", "Model Cache Invalidation"),
        ],
        string="Reason",
        required=True,
        index=True,
    )

    create_date = fields.Datetime(index=True)

    @api.model_create_multi
    def create(self, vals_list):
        clean_ctx = dict(self.env.context)
        clean_ctx.pop("mail_notrack", None)
        clean_ctx.pop("prefetch_fields", None)
        return super(SecurityLog, self.with_context(**clean_ctx)).create(vals_list)

    @api.model
    def autovacuum(self):
        ninety_days_ago = fields.Datetime.now() - datetime.timedelta(days=90)
        self.env.cr.execute(
            """
            DELETE FROM zero_sudo_security_log
            WHERE id IN (
                SELECT id FROM zero_sudo_security_log
                WHERE create_date < %s
                LIMIT 10000
            )
            """,
            (ninety_days_ago,)
        )
