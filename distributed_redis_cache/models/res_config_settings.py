# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import fields, models

from odoo.addons.distributed_redis_cache import redis_pool as _redis_pool_module


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

    def set_values(self):
        # bug-hunt (2026-09-09): redis_pool.get_redis_connection() caches
        # the resolved (host, port, password) tuple per dbname in the
        # module-level `_db_configs` dict FOREVER, with nothing anywhere
        # ever invalidating it (confirmed by grep: no other write site
        # touches `_db_configs`). Before this fix, saving new Redis
        # settings here had literally zero effect on any already-running
        # worker until it was restarted -- including the "Check Redis
        # Status" button on distributed.cache.config, whose whole purpose
        # is to verify the config an admin just changed, but which would
        # silently keep testing the OLD cached config instead.
        #
        # KNOWN LIMITATION, left as-is deliberately rather than solved
        # here: `_db_configs` is a plain process-local dict, not a
        # cross-worker cache. Popping this worker's own entry only fixes
        # the request that lands on the SAME worker that handled the
        # save; other worker processes still hold the stale tuple until
        # they happen to be restarted. This module already accepts an
        # equivalent restart-required caveat for the cache_manager.py
        # daemon itself (see hooks.py's post_init_hook comment) -- fixing
        # this cross-worker for real would mean propagating the change
        # via the existing pg_notify/global-counter mechanism
        # (notify_model_invalidation) and is a bigger change than this
        # bug-hunt pass's scope; still a strict improvement over "never,
        # on any worker, until every worker is restarted."
        res = super().set_values()
        with _redis_pool_module.POOL_LOCK:
            _redis_pool_module._db_configs.pop(self.env.cr.dbname, None)
        return res
