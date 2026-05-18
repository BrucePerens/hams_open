# -*- coding: utf-8 -*-
import odoo
from odoo.modules.registry import Registry
from . import redis_pool
from . import redis_cache
from . import models
from .hooks import post_init_hook
from odoo.addons.distributed_redis_cache.redis_cache import _local_cache

# Silence flake8 unused import warnings
__all__ = ["redis_pool", "redis_cache", "models", "post_init_hook"]

# Hook into Odoo's registry cache clearing to ensure our local fallback cache
# stays synchronized during test rollbacks and global clear events.
_original_clear_cache = Registry.clear_cache


def _new_clear_cache(self, *args, **kwargs):
    _local_cache.clear()
    return _original_clear_cache(self, *args, **kwargs)


Registry.clear_cache = _new_clear_cache

# Hook into Odoo's test teardown to purge the cache between test transactions.
# This prevents test pollution since the local fallback cache isn't natively
# bound to savepoints.
if odoo.tools.config.get("test_enable"):
    import odoo.tests.common

    _orig_tearDown = odoo.tests.common.BaseCase.tearDown

    def _new_tearDown(self):
        _local_cache.clear()
        return _orig_tearDown(self)

    odoo.tests.common.BaseCase.tearDown = _new_tearDown
