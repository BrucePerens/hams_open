# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
import json

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

    # [@ANCHOR: distributed_redis_cache:res_config_settings_set_values]
    # Verified by [@ANCHOR: test_b2_10_settings_save_invalidates_the_redis_pool_cache]
    def set_values(self):
        # redis_pool.get_redis_connection() caches the resolved
        # (host, port, password) tuple per dbname in the module-level
        # `_db_configs` dict, and nothing else ever invalidates it (no other
        # write site touches `_db_configs`). So saving new Redis settings
        # here must clear it, or the change would have no effect on any
        # already-running worker until it was restarted -- including the
        # "Check Redis Status" button on distributed.cache.config, whose
        # whole purpose is to verify the config an admin just changed, but
        # which would silently keep testing the OLD cached config instead.
        #
        # `_db_configs` is a plain process-local dict, not a cross-worker
        # cache -- popping this worker's own entry only covers the request
        # that landed on the SAME worker that handled the save; other worker
        # processes would keep the stale tuple until restarted. So the
        # invalidation is propagated cross-worker via the same pg_notify -> cache_manager.py -> Redis
        # global_cache_invalidation_counter path `notify_model_invalidation()` uses for real
        # model-cache invalidation. Not `notify_model_invalidation()` itself: it requires a real
        # Odoo model name (`model_name not in env` is a hard fail), and `_db_configs` isn't
        # model-backed data at all -- this fires the identical raw pg_notify shape by hand
        # instead. `poll_and_clear_local_cache()` (redis_cache.py) is what every OTHER worker
        # already runs on every request/cron dispatch to notice the counter changed; it also
        # clears `_db_configs` there, not just `_local_cache`.
        res = super().set_values()
        dbname = self.env.cr.dbname
        _redis_pool_module.clear_db_config_cache(dbname)
        payload = json.dumps(
            {"model": "distributed_redis_cache.res_config_settings", "dbname": dbname}
        )
        self.env.cr.execute(
            "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
        )
        return res
