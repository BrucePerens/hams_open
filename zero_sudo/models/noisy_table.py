# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, fields


class NoisyTable(models.Model):
    # [@ANCHOR: COMM_zero_sudo_noisy_table_global]
    # This model is logically GLOBAL and NOT multi-tenanted.
    # It contains the names of physical PostgreSQL tables that should be ignored
    # by the leak detection engine. Physical tables are global to the database cluster.
    _name = "zero_sudo.noisy_table"
    _description = "Noisy Table"
    _order = "name"

    name = fields.Char(
        string="Table Name",
        required=True,
        index=True,
        help=(
            "Name of the PostgreSQL table to ignore in "
            "leak detection."
        ),
    )
    active = fields.Boolean(
        default=True,
        index=True,
        help=(
            "If unchecked, it will allow leak "
            "detection for this table."
        ),
    )

    _name_uniq = models.Constraint("UNIQUE(name)", "The table name must be unique!")
