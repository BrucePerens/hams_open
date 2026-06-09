# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
from odoo import fields, models


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    redis_host = fields.Char(
        string="Redis Host",
        config_parameter="distributed_redis_cache.redis_host",
        default="redis",
    )
    redis_port = fields.Integer(
        string="Redis Port",
        config_parameter="distributed_redis_cache.redis_port",
        default=6379,
    )
    redis_password = fields.Char(
        string="Redis Password",
        config_parameter="distributed_redis_cache.redis_password",
    )
