# -*- coding: utf-8 -*-
from odoo import models, fields


class SecurityLog(models.Model):
    # [@ANCHOR: zero_sudo_security_log_global]
    # This model is logically GLOBAL and NOT multi-tenanted.
    # It records blocked service account login attempts for auditing.
    _name = "zero_sudo.security.log"
    _description = "Zero-Sudo Security Audit Log"
    _order = "create_date desc"

    user_id = fields.Many2one(
        "res.users", string="Target User", required=True, index=True
    )
    login = fields.Char(string="Login Used", index=True)
    ip_address = fields.Char(string="IP Address", index=True)
    user_agent = fields.Char(string="User Agent")
    reason = fields.Selection(
        [
            ("service_account_blocked", "Service Account Web Login Attempt"),
            ("god_mode_trip", "God-Mode Security Block Tripped"),
            ("param_access_denied", "Unauthorized System Parameter Access"),
            ("param_write_denied", "Unauthorized System Parameter Write"),
            ("cache_invalidation", "Model Cache Invalidation"),
        ],
        string="Reason",
        required=True,
    )
