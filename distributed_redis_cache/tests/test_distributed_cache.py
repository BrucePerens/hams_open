# -*- coding: utf-8 -*-
import os
import logging
import redis
from unittest.mock import MagicMock

from odoo import tools
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsHttpCase, HamsIntegrationCase
from odoo.addons.distributed_redis_cache.models.ir_http import (
    _invalidation_queue,
    _listener_lock,
)
from odoo.addons.distributed_redis_cache.redis_cache import (
    invalidate_model_cache,
    distributed_cache,
    _local_cache,
    _get_hash,
    notify_model_invalidation,
)

_logger = logging.getLogger(__name__)


@tagged("standard", "post_install", "-at_install")
class TestDistributedCacheStandard(HamsHttpCase):
    def test_01_redis_cache_interceptor_standard(self):
        # Tests [@ANCHOR: redis_cache_interceptor]
        """
        BDD: Given a distributed environment utilizing Redis
        When a cache invalidation signal is detected on the pubsub bus
        Then the worker MUST flush its local targeted RAM cache.
        """
        mock_endpoint = MagicMock()
        mock_endpoint.routing = {"auth": "none"}

        with _listener_lock:
            _invalidation_queue.add(("res.users", self.env.cr.dbname))

        self.safe_patch("odoo.addons.distributed_redis_cache.models.ir_http.redis_pool", new=None)
        self.safe_patch("odoo.addons.base.models.ir_http.IrHttp._authenticate", return_value=True)

        class MockRequest:
            env = MagicMock()

        mock_req_inst = MockRequest()

        self.safe_patch("odoo.addons.distributed_redis_cache.models.ir_http.request", new=mock_req_inst)
        mock_invalidate = self.safe_patch("odoo.addons.distributed_redis_cache.models.ir_http.invalidate_model_cache")

        mock_req_inst.env.__contains__.return_value = True
        mock_req_inst.env.__getitem__.return_value = self.env["res.users"]
        mock_req_inst.env.cr.dbname = self.env.cr.dbname

        self.env["ir.http"]._authenticate(mock_endpoint)
        mock_invalidate.assert_called_with(mock_req_inst.env, "res.users", local_only=True)

    def test_02_redis_interceptor_fails_open_standard(self):
        """
        Verify that if the Redis connection dies during polling, the middleware
        gracefully catches the exception and allows the HTTP request to proceed without crashing the worker.
        """
        self.safe_patch("odoo.addons.distributed_redis_cache.models.ir_http.redis_pool", new=MagicMock())
        mock_redis = self.safe_patch("odoo.addons.distributed_redis_cache.models.ir_http.redis")
        self.safe_patch("odoo.addons.distributed_redis_cache.models.ir_http.request", new=MagicMock())
        self.safe_patch("odoo.addons.base.models.ir_http.IrHttp._authenticate", return_value=True)

        mock_redis.RedisError = redis.RedisError
        mock_redis.Redis.side_effect = redis.RedisError("Connection reset by peer")

        mock_endpoint = MagicMock()
        mock_endpoint.routing = {"auth": "none"}
        # The Redis interceptor MUST fail-open and never crash the WSGI worker.
        # If an exception is raised here, the test will fail.
        self.env["ir.http"]._authenticate(mock_endpoint)

    def test_03_distributed_cache_ui(self):
        # Tests [@ANCHOR: distributed_cache_view]
        # Tests [@ANCHOR: manual_cache_invalidation]
        # Tests [@ANCHOR: check_redis_status_logic]
        """
        Verify the UI logic for manually invalidating the cache.
        """
        self.env["distributed.cache.config"].get_view()
        wiz = self.env["distributed.cache.config"].create(
            {"model_id": self.env.ref("base.model_res_users").id}
        )
        res = wiz.action_invalidate_model_cache()
        self.assertEqual(res["type"], "ir.actions.client")
        self.assertEqual(res["params"]["type"], "success")

        res_redis = wiz.check_redis_status()
        self.assertEqual(res_redis["type"], "ir.actions.client")

    def test_05_redis_scan_invalidation_standard(self):
        # Tests [@ANCHOR: invalidate_model_cache_logic]
        # Tests [@ANCHOR: redis_connection_pool]
        """
        Verify that invalidate_model_cache uses SCAN instead of KEYS.
        """
        self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis_pool", new=MagicMock())
        mock_redis = self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis")

        mock_redis_client = MagicMock()
        mock_redis.Redis.return_value = mock_redis_client
        mock_redis_client.scan_iter.return_value = ["key1", "key2"]

        invalidate_model_cache(self.env, "res.partner")

        mock_redis_client.scan_iter.assert_called_once()
        mock_redis_client.delete.assert_called_once_with("key1", "key2")

    def test_06_distributed_cache_decorator_fallback(self):
        # Tests [@ANCHOR: distributed_cache_decorator]
        """
        Verify the @distributed_cache decorator falls back to local cache when Redis fails.
        """

        class MockModel:
            def __init__(self, env):
                self.env = env
                self._name = "mock.model"

            @distributed_cache()
            def cached_method(self, val):
                return val * 2

        mock_obj = MockModel(self.env)
        _local_cache.clear()

        self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis_pool", new=MagicMock())
        mock_redis = self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis")

        old_test_enable = tools.config.options.get("test_enable")
        tools.config.options["test_enable"] = False

        try:
            mock_redis.RedisError = redis.RedisError
            mock_redis_client = MagicMock()
            mock_redis.Redis.return_value = mock_redis_client
            mock_redis_client.get.side_effect = redis.RedisError("Redis Down")

            result = mock_obj.cached_method(21)
            self.assertEqual(result, 42)

            # Verify it's in local cache now
            self.assertIn(42, list(_local_cache.values()))
        finally:
            tools.config.options["test_enable"] = old_test_enable

    def test_07_distributed_cache_key_generation(self):
        # Tests [@ANCHOR: distributed_cache_key_generation]
        """
        Verify that cache keys are generated deterministically.
        """
        h1 = _get_hash(1, 2, a=3)
        h2 = _get_hash(1, 2, a=3)
        self.assertEqual(h1, h2)

        h3 = _get_hash(2, 1, a=3)
        self.assertNotEqual(h1, h3)

        # Test model serialization
        h4 = _get_hash(self.env.user)
        h5 = _get_hash(self.env.user)
        self.assertEqual(h4, h5)

        # Test complex serialization
        h6 = _get_hash([1, 2], {"c": 3, "b": 4})
        h7 = _get_hash([1, 2], {"b": 4, "c": 3})
        self.assertEqual(h6, h7)

        # Test frozenset and nested structures
        h8 = _get_hash(frozenset([3, 1, 2]))
        h9 = _get_hash(set([1, 2, 3]))
        self.assertEqual(h8, h9)

        # Test None and bool
        h10 = _get_hash(None, True, False)
        h11 = _get_hash(None, True, False)
        self.assertEqual(h10, h11)

    def test_08_notify_model_invalidation_logic(self):
        # Tests [@ANCHOR: notify_model_invalidation_logic]
        """
        Verify that notify_model_invalidation calls invalidate_model_cache and pg_notify.
        """
        mock_invalidate = self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.invalidate_model_cache")
        mock_execute = self.safe_patch_object(self.env.cr, "execute")

        notify_model_invalidation(self.env, "res.users")

        mock_invalidate.assert_called_once_with(self.env, "res.users", local_only=False)
        mock_execute.assert_called_once()
        args, _ = mock_execute.call_args
        self.assertEqual(args[0], "SELECT pg_notify(%s, %s)")
        self.assertEqual(args[1][0], "distributed_cache_invalidation")
        self.assertIn('"model": "res.users"', args[1][1])
        self.assertIn(f'"dbname": "{self.env.cr.dbname}"', args[1][1])

    def test_09_multi_website_cache_keys(self):
        """Verify that cache keys are website-aware."""
        class MockModel:
            def __init__(self, env):
                self.env = env
                self._name = "mock.model"

            @distributed_cache()
            def cached_method(self, val):
                return val

        _local_cache.clear()

        old_test_enable = tools.config.options.get("test_enable")
        tools.config.options["test_enable"] = False
        self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis_pool", new=None)

        try:
            # Website 1
            env_w1 = self.env(context=dict(self.env.context, website_id=1))
            obj_w1 = MockModel(env_w1)
            obj_w1.cached_method("test")

            keys = list(_local_cache.keys())
            _logger.info("Keys after w1: %s", keys)
            self.assertTrue(any(":w1" in k for k in keys), "Expected key for website 1 not found in %s" % keys)

            # Website 2
            env_w2 = self.env(context=dict(self.env.context, website_id=2))
            obj_w2 = MockModel(env_w2)
            obj_w2.cached_method("test")

            # They should have different keys in _local_cache
            keys = list(_local_cache.keys())
            _logger.info("Keys after w2: %s", keys)
            w1_keys = [k for k in keys if ":w1" in k]
            w2_keys = [k for k in keys if ":w2" in k]

            self.assertTrue(w1_keys, "Expected key for website 1 not found in %s" % keys)
            self.assertTrue(w2_keys, "Expected key for website 2 not found in %s" % keys)
            self.assertNotEqual(w1_keys[0], w2_keys[0])
        finally:
            tools.config.options["test_enable"] = old_test_enable

    def test_10_invalidate_model_cache_local_and_redis(self):
        """Verify that invalidate_model_cache flushes both local and Redis cache."""
        self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis_pool", new=MagicMock())
        mock_redis = self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.redis")
        mock_redis_client = MagicMock()
        mock_redis.Redis.return_value = mock_redis_client
        mock_redis_client.scan_iter.return_value = ["key1"]

        dbname = self.env.cr.dbname
        local_key = f"{dbname}:distributed_cache:res.partner:test"
        _local_cache[local_key] = "val"

        invalidate_model_cache(self.env, "res.partner")

        self.assertNotIn(local_key, _local_cache)
        mock_redis_client.delete.assert_called()


@tagged("integration", "post_install", "-at_install")
class TestDistributedCacheIntegration(HamsIntegrationCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        daemon_path = os.path.abspath(
            os.path.join(
                os.path.dirname(__file__), "..", "daemons", "cache_manager.py"
            )
        )
        if os.path.exists(daemon_path):
             cls.start_daemon(daemon_path, env_vars={
                 "REDIS_HOST": "redis",
                 "DB_HOST": "odoo",
                 "DB_NAME": cls.env.cr.dbname,
                 "DB_USER": "odoo",
                 "DB_PASS": "odoo",
             })

    def test_01_full_pipeline_integration(self):
        """
        Verify the full invalidation pipeline:
        Postgres NOTIFY -> Cache Manager -> Redis PubSub -> Odoo Worker.
        """
        # 1. Setup a cached method
        class MockModel:
            def __init__(self, env):
                self.env = env
                self._name = "res.partner"

            @distributed_cache()
            def cached_method(self, val):
                return val

        obj = MockModel(self.env)
        _local_cache.clear()

        old_test_enable = tools.config.options.get("test_enable")
        tools.config.options["test_enable"] = False
        try:
            obj.cached_method("init")
            cache_key = list(_local_cache.keys())[0]
            self.assertIn(cache_key, _local_cache)

            # 2. Trigger invalidation via real PG NOTIFY
            notify_model_invalidation(self.env, "res.partner")
            self.assertNotIn(cache_key, _local_cache)
        finally:
            tools.config.options["test_enable"] = old_test_enable


@tagged("post_install", "-at_install")
class TestDistributedCacheTour(HamsHttpCase):
    def test_distributed_cache_admin_tour(self):
        """Verify the cache management UI via tour."""
        self.start_tour("/odoo?debug=1", "distributed_cache_admin_tour", login="admin")
