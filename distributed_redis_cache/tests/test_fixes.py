# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, _local_cache
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection, _custom_pools
import odoo.addons.distributed_redis_cache.redis_cache as rc
import odoo.addons.distributed_redis_cache.redis_pool as rp
from odoo import fields


class DummyModel:
    def __init__(self, ids=None):
        self.ids = ids or []
        self._name = "dummy.model"

        class Cr:
            dbname = "test_db"

        class Env(dict):
            def __init__(self):
                self.cr = Cr()
                self.context = {}

        self.env = Env()

    @distributed_cache()
    def cached_method(self, x):
        return x

    @distributed_cache()
    def cached_method_datetime(self):
        return fields.Datetime.now()

@tagged("-at_install", "post_install")
class TestDistributedRedisCacheFixes(HamsTransactionCase):

    def test_cache_key_poisoning(self):
        """Test that different recordsets get different cache keys."""
        model1 = DummyModel(ids=[1])
        model2 = DummyModel(ids=[2])
        _local_cache.clear()
        
        model1.cached_method("same_arg")
        model2.cached_method("same_arg")
        self.assertEqual(len(_local_cache), 2, "Should have 2 different cache keys for different ids.")

    def test_serialization(self):
        """Test that datetime objects can be serialized."""
        model = DummyModel(ids=[1])
        class FakeRedis:
            def get(self, key): return None
            def setex(self, key, ttl, val): self.val = val
        
        mock_get_conn = self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection")
        mock_get_conn.return_value = FakeRedis()
        try:
            model.cached_method_datetime()
        except TypeError as e:
            self.fail(f"Serialization failed with TypeError: {e}")

    def test_thread_safety_local_cache(self):
        """Test that local cache accesses use LRU_LOCK."""
        model = DummyModel(ids=[1])
        _local_cache.clear()

        mock_lock = self.safe_patch_object(rc, 'LRU_LOCK')
        model.cached_method("test")
        self.assertTrue(mock_lock.__enter__.called, "LRU_LOCK was not used")

    def test_thread_safety_redis_pool(self):
        """Test that redis pool initialization is thread safe."""
        _custom_pools.clear()

        class MockSecurityUtils:
            def _get_system_param(self, key, default):
                if "host" in key: return "custom_host"
                if "port" in key: return "6380"
                if "password" in key: return "pass"
                return default

        class MockEnv(dict):
            def __init__(self):
                self["zero_sudo.security.utils"] = MockSecurityUtils()

        env = MockEnv()
        
        mock_lock = self.safe_patch_object(rp, 'POOL_LOCK')
        get_redis_connection(env)
        self.assertTrue(mock_lock.__enter__.called, "POOL_LOCK was not used")
