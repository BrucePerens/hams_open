# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import models, fields


class ResUsers(models.Model):
    _inherit = 'res.users'

    daemon_registry_ids = fields.One2many(
        'daemon.key.registry',
        'user_id',
        string="Daemon Registries"
    )
