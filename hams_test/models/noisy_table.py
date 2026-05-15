# -*- coding: utf-8 -*-
from odoo import models, fields, api

class NoisyTable(models.Model):
    _name = 'test_real_transaction.noisy_table'
    _description = 'Noisy Table'

    name = fields.Char(string='Table Name', required=True, help='Name of the PostgreSQL table to ignore in leak detection.')
    active = fields.Boolean(default=True, help='If unchecked, it will allow leak detection for this table.')

    _name_uniq = models.Constraint('UNIQUE(name)', 'The table name must be unique!')
