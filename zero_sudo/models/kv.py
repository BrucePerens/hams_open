# -*- coding: utf-8 -*-
from odoo import models, fields


class ZeroSudoKV(models.Model):
    # [@ANCHOR: zero_sudo_kv_global]
    # This model is logically GLOBAL and NOT multi-tenanted.
    # It stores low-level system state and cryptographic flags for service accounts
    # that operate across all companies and websites.
    _name = "zero_sudo.kv"
    _description = "Zero-Sudo Key-Value Store"

    key = fields.Char(string="Key", required=True, index=True)
    value = fields.Text(string="Value")

    _key_uniq = models.Constraint(
        "UNIQUE(key)", "The key must be mathematically unique."
    )
