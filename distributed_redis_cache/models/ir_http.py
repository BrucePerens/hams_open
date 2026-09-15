# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from odoo import models, tools
from odoo.http import request

from odoo.addons.distributed_redis_cache.redis_cache import poll_and_clear_local_cache

_logger = logging.getLogger(__name__)

class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: COMM_redis_cache_interceptor]
        """
        Intercepts request lifecycle to check cache invalidation.
        """
        init_mode = tools.config.get("init")
        update_mode = tools.config.get("update")
        stop_after_init = tools.config.get("stop_after_init")

        if not (init_mode or update_mode or stop_after_init):
            poll_and_clear_local_cache(request.env)

        return super()._authenticate(endpoint)
