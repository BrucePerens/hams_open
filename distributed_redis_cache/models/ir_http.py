# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from odoo import models
from odoo.http import request

from odoo.addons.distributed_redis_cache.redis_cache import (
    poll_and_clear_local_cache,
    should_poll_for_invalidation,
)

_logger = logging.getLogger(__name__)

class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: COMM_redis_cache_interceptor]
        """
        Intercepts request lifecycle to check cache invalidation.
        """
        # `cls.pool` is this request's own registry (every registry-composed
        # model class carries one). See COMM_should_poll_for_invalidation for
        # why the gate is the registry's own loading state and not
        # `tools.config`'s never-cleared `init`/`update` flags.
        if should_poll_for_invalidation(cls.pool):
            poll_and_clear_local_cache(request.env)

        return super()._authenticate(endpoint)
