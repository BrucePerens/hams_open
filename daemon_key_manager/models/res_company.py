# -*- coding: utf-8 -*-
from odoo import models, fields


class ResCompany(models.Model):
    _inherit = 'res.company'

    daemon_registry_ids = fields.One2many(
        'daemon.key.registry',
        'company_id',
        string="Daemon Registries"
    )
