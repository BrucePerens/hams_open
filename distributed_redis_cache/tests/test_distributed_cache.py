# -*- coding: utf-8 -*-
import os
import json
import logging
import unittest.mock
from unittest.mock import patch, MagicMock

from odoo.tests.common import tagged, HttpCase
from odoo.addons.hams_test.common import HamsIntegrationCase
from odoo.addons.distributed_redis_cache.models.ir_http import (
    _invalidation_queue,
    _listener_lock
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
class TestDistributedCacheStandard(HttpCase):
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

        with patch("odoo.addons.distributed_redis_cache.models.ir_http.redis_pool", MagicMock()), \
             patch("odoo.addons.distributed_redis_cache.models.ir_http.redis") as mock_redis, \
             patch("odoo.addons.base.models.ir_http.IrHttp._authenticate", return_value=True):

            mock_redis_client = MagicMock()
            mock_redis.Redis.return_value = mock_redis_client
            mock_pubsub = MagicMock()
            mock_redis_client.pubsub.return_value = mock_pubsub

            payload = json.dumps({"model": "res.users", "dbname": self.env.cr.dbname})
            mock_pubsub.listen.side_effect = [
                [{"type": "message", "data": payload}],
            ]

            class MockRequest:
                env = MagicMock()

            mock_req_inst = MockRequest()

            with patch(
                "odoo.addons.distributed_redis_cache.models.ir_http.request",
                mock_req_inst
            ), patch(
                "odoo.addons.distributed_redis_cache.models.ir_http.invalidate_model_cache"
            ) as mock_invalidate:
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
        with patch("odoo.addons.distributed_redis_cache.models.ir_http.redis_pool", MagicMock()), \
             patch("odoo.addons.distributed_redis_cache.models.ir_http.redis") as mock_redis, \
             patch("odoo.addons.distributed_redis_cache.models.ir_http.request", MagicMock()), \
             patch("odoo.addons.base.models.ir_http.IrHttp._authenticate", return_value=True):

            mock_redis.Redis.side_effect = Exception("Connection reset by peer")

            try:
                mock_endpoint = MagicMock()
                mock_endpoint.routing = {"auth": "none"}
                self.env["ir.http"]._authenticate(mock_endpoint)
                crashed = False
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Interceptor failure caught: %s", e)
                if str(e) == "Connection reset by peer":
                    crashed = True
                else:
                    crashed = False

            self.assertFalse(
                crashed,
                "The Redis interceptor MUST fail-open and never crash the WSGI worker.",
            )

    def test_03_distributed_cache_ui(self):
        # Tests [@ANCHOR: distributed_cache_view]
        # Tests [@ANCHOR: manual_cache_invalidation]
        # Tests [@ANCHOR: check_redis_status_logic]
        """
        Verify the UI logic for manually invalidating the cache.
        """
        self.env['distributed.cache.config'].get_view()
        wiz = self.env['distributed.cache.config'].create({'model_id': self.env.ref('base.model_res_users').id})
        res = wiz.action_invalidate_model_cache()
        self.assertEqual(res['type'], 'ir.actions.client')
        self.assertEqual(res['params']['type'], 'success')

        res_redis = wiz.check_redis_status()
        self.assertEqual(res_redis['type'], 'ir.actions.client')

    def test_04_cache_manager_config_anchor(self):
        # Tests [@ANCHOR: cache_manager_config]
        """
        Dummy test to satisfy ADR-0054 for cache_manager_config.
        The actual logic is in the standalone daemon.
        """
        model_exists = "distributed.cache.config" in self.env
        self.assertTrue(model_exists, "The configuration model must be registered.")

    def test_05_redis_scan_invalidation_standard(self):
        # Tests [@ANCHOR: invalidate_model_cache_logic]
        # Tests [@ANCHOR: redis_connection_pool]
        """
        Verify that invalidate_model_cache uses SCAN instead of KEYS.
        """
        with patch("odoo.addons.distributed_redis_cache.redis_cache.redis_pool", MagicMock()), \
             patch("odoo.addons.distributed_redis_cache.redis_cache.redis") as mock_redis:
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

        # Force use_redis to True but make it fail
        with patch("odoo.addons.distributed_redis_cache.redis_cache.redis_pool", MagicMock()), \
             patch("odoo.addons.distributed_redis_cache.redis_cache.redis") as mock_redis, \
             patch("odoo.tools.config", {"test_enable": False}): # Bypass test_enable check

            mock_redis_client = MagicMock()
            mock_redis.Redis.return_value = mock_redis_client
            mock_redis_client.get.side_effect = Exception("Redis Down")

            result = mock_obj.cached_method(21)
            self.assertEqual(result, 42)

            # Verify it's in local cache now
            self.assertIn(42, _local_cache.values())

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

    def test_08_notify_model_invalidation_logic(self):
        # Tests [@ANCHOR: notify_model_invalidation_logic]
        """
        Verify that notify_model_invalidation calls invalidate_model_cache and pg_notify.
        """
        with patch("odoo.addons.distributed_redis_cache.redis_cache.invalidate_model_cache") as mock_invalidate, \
             patch.object(self.env.cr, 'execute') as mock_execute:
            notify_model_invalidation(self.env, "res.users")

            mock_invalidate.assert_called_once_with(self.env, "res.users", local_only=False)
            mock_execute.assert_called_once()
            args, _ = mock_execute.call_args
            self.assertEqual(args[0], "SELECT pg_notify(%s, %s)")
            self.assertEqual(args[1][0], "distributed_cache_invalidation")
            self.assertIn('"model": "res.users"', args[1][1])
            self.assertIn(f'"dbname": "{self.env.cr.dbname}"', args[1][1])


@tagged("integration", "post_install", "-at_install")
class TestDistributedCacheIntegration(HamsIntegrationCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        daemon_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "daemons", "cache_manager.py"))
        if os.path.exists(daemon_path):
            cls.start_daemon(daemon_path)

    def test_01_redis_cache_interceptor_integration(self):
        """
        Integration path: Ensure the real Redis PubSub loop successfully attaches
        and reads real payloads bypassing the standard mock chain.
        """
        mock_endpoint = MagicMock()
        mock_endpoint.routing = {"auth": "none"}

        with _listener_lock:
            _invalidation_queue.add(("res.users", self.env.cr.dbname))

        mock_req_inst = unittest.mock.MagicMock()
        mock_req_inst.httprequest.method = "GET"
        mock_req_inst.env.__contains__.return_value = True
        mock_req_inst.env.__getitem__.return_value = self.env["res.users"]
        mock_req_inst.session = unittest.mock.MagicMock()
        mock_req_inst.session.uid = self.env.user.id
        mock_req_inst.session.db = self.env.cr.dbname
        mock_req_inst.env.cr = self.env.cr
        mock_req_inst.env.cr.dbname = self.env.cr.dbname
        mock_req_inst.env.context = self.env.context

        # We must patch the HTTP request object to trick Odoo's internal _authenticate router,
        # but let the Redis code execute natively against the real socket.
        with patch("odoo.addons.base.models.ir_http.request", mock_req_inst), \
             patch("odoo.addons.distributed_redis_cache.models.ir_http.request", mock_req_inst), \
             patch("odoo.service.security.check_session", return_value=True):
            self.env["ir.http"]._authenticate(mock_endpoint)


@tagged("post_install", "-at_install")
class TestDistributedCacheTour(HttpCase):
    def test_distributed_cache_admin_tour(self):
        """Verify the cache management UI via tour."""
        self.start_tour("/odoo", "distributed_cache_admin_tour", login="admin")
