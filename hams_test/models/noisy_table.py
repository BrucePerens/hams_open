# -*- coding: utf-8 -*-
from odoo import models, fields, api

class NoisyTable(models.Model):
    _name = 'test_real_transaction.noisy_table'
    _description = 'Noisy Table'

    name = fields.Char(string='Table Name', required=True, help='Name of the PostgreSQL table to ignore in leak detection.')
    active = fields.Boolean(default=True, help='If unchecked, it will allow leak detection for this table.')

    _name_uniq = models.Constraint('UNIQUE(name)', 'The table name must be unique!')

    @api.model
    def _register_hook(self):
        # [@ANCHOR: documentation_bootstrap]
        # Verified by [@ANCHOR: test_documentation_bootstrap]
        super()._register_hook()
        self._install_hams_test_docs()

    @api.model
    def _install_hams_test_docs(self):
        # [@ANCHOR: documentation_injection]
        # Verified by [@ANCHOR: test_documentation_injection]
        # This module uses the zero_sudo generic documentation installer.
        # We trigger a check here to ensure it's loaded as soon as this module is ready.
        if "ir.module.module" in self.env:
            self.env["ir.module.module"]._bootstrap_knowledge_docs()
