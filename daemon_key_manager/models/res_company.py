# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields


class ResCompany(models.Model):
    _inherit = 'res.company'

    daemon_registry_ids = fields.One2many(
        'daemon.key.registry',
        'company_id',
        string="Daemon Registries"
    )
