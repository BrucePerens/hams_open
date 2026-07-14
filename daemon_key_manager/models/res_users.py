# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields


class ResUsers(models.Model):
    _inherit = 'res.users'

    daemon_registry_ids = fields.One2many(
        'daemon.key.registry',
        'user_id',
        string="Daemon Registries"
    )
