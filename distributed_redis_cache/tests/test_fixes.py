# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, _local_cache
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection, _custom_pools
import odoo.addons.distributed_redis_cache.redis_cache as rc
import odoo.addons.distributed_redis_cache.redis_pool as rp
from odoo import fields
import asyncio
import odoo.addons.distributed_redis_cache.daemons.cache_manager as cm
from odoo.addons.distributed_redis_cache.daemons.cache_manager import broadcast_to_redis, postgres_notify_handler


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
            def get(self, key):
                return None

            def setex(self, key, ttl, val):
                self.val = val
        
        mock_get_conn = self.safe_patch("odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection")
        mock_get_conn.return_value = FakeRedis()
        try:
            model.cached_method_datetime()
        except TypeError as e:
            self.fail(f"Serialization failed with TypeError: {e}")

    def test_forged_pickle_payload_is_rejected_not_deserialized(self):
        """
        [!] SECURITY: cache values are HMAC-signed before being written to
        Redis specifically so that a payload no attacker without our
        crypto secret could have produced (e.g. planted directly in Redis
        by a network peer with Redis access but not app access) is never
        passed to _pickle.loads(). Prove a forged payload is rejected by
        checking it never reaches the unsafe deserializer, and that the
        decorator falls back to recomputing the real function instead of
        raising or trusting the forged value.
        """
        model = DummyModel(ids=[1])

        forged_marker = "PWNED_VIA_FORGED_PICKLE"
        real_unpickle = rc._pickle.loads
        forged_was_deserialized = []

        def spy_loads(data, *args, **kwargs):
            forged_was_deserialized.append(data)
            return real_unpickle(data, *args, **kwargs)

        self.safe_patch(
            "odoo.addons.distributed_redis_cache.redis_cache._pickle.loads",
            side_effect=spy_loads,
        )

        # An attacker who can write to Redis but doesn't know our crypto
        # secret can still pickle a *valid* payload -- what they can't do
        # is produce a signature that verifies. Simulate exactly that:
        # correctly pickled bytes, wrong signature.
        forged_pickle_hex = rc._pickle.dumps(forged_marker).hex()
        forged_stored_value = f"deadbeef{'0' * 56}:{forged_pickle_hex}"

        class FakeRedis:
            def get(self, key):
                return forged_stored_value

            def setex(self, key, ttl, val):
                pass

        mock_get_conn = self.safe_patch(
            "odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection"
        )
        mock_get_conn.return_value = FakeRedis()

        result = model.cached_method("real_value")

        self.assertEqual(
            result,
            "real_value",
            "A forged cache payload must never be trusted -- the "
            "decorator must fall back to recomputing the real function.",
        )
        self.assertFalse(
            forged_was_deserialized,
            "_pickle.loads() must never be called on a payload whose "
            "HMAC signature didn't verify -- that's the whole point of "
            "signing cache payloads.",
        )

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
                if "host" in key:
                    return "custom_host"
                if "port" in key:
                    return "6380"
                if "password" in key:
                    return "pass"
                return default
                
            def with_context(self, **kwargs):
                return self

        class MockEnv(dict):
            def __init__(self):
                self["zero_sudo.security.utils"] = MockSecurityUtils()
                self.cr = type("cr", (), {"dbname": "test"})()

        env = MockEnv()
        
        mock_lock = self.safe_patch_object(rp, 'POOL_LOCK')
        get_redis_connection(env)
        self.assertTrue(mock_lock.__enter__.called, "POOL_LOCK was not used")

    def test_cache_manager_exception_handling(self):
        # [@ANCHOR: COMM_test_cache_manager_exception_handling]
        """Test exception handling in cache manager broadcast_to_redis."""
        
        class FakePipeline:
            def __init__(self):
                self.executed = False
            async def __aenter__(self): return self
            async def __aexit__(self, exc_type, exc_val, exc_tb): pass
            def publish(self, channel, payload): return self
            def incr(self, key): return self
            async def execute(self): 
                self.executed = True
                raise ValueError("Intentional fake exception")

        class FakeRedisClient:
            def __init__(self):
                self.pipe = FakePipeline()
            def pipeline(self):
                return self.pipe
        
        cm.redis_client = FakeRedisClient()
        try:
            asyncio.run(broadcast_to_redis('{"model": "res.users", "dbname": "test"}'))
            self.assertTrue(cm.redis_client.pipe.executed, "Should swallow exception")
        except ValueError:
            self.fail("ValueError was not caught by audit-ignore-catch-all")


    def test_cache_manager_broadcast_invalid_json_type(self):
        """Test that broadcast_to_redis handles non-dict JSON gracefully."""

        class FakePipeline:
            async def __aenter__(self): return self
            async def __aexit__(self, exc_type, exc_val, exc_tb): pass
            def publish(self, channel, payload): return self
            def incr(self, key): return self
            async def execute(self): pass

        class FakeRedisClient:
            def pipeline(self):
                return FakePipeline()
            async def publish(self, channel, payload): pass
            async def incr(self, key): pass

        cm.redis_client = FakeRedisClient()
        # Should not raise AttributeError when data is a list
        asyncio.run(broadcast_to_redis('["model", "res.users"]'))

    def test_cache_manager_broadcast_pipeline(self):
        """Test that broadcast_to_redis uses redis pipeline."""

        class FakePipeline:
            async def __aenter__(self):
                return self
            async def __aexit__(self, exc_type, exc_val, exc_tb):
                pass
            def publish(self, channel, payload):
                return self
            def incr(self, key):
                return self
            async def execute(self):
                self.executed = True

        class FakeRedisClient:
            def __init__(self):
                self.pipe = FakePipeline()
            def pipeline(self):
                return self.pipe
            async def publish(self, channel, payload):
                pass
            async def incr(self, key):
                pass

        cm.redis_client = FakeRedisClient()
        asyncio.run(broadcast_to_redis('{"model": "res.users", "dbname": "test"}'))
        try:
            self.assertTrue(cm.redis_client.pipeline().executed, 'Redis pipeline was not executed')
        except AssertionError:
            self.fail('Redis pipeline was not executed')

    def test_cache_manager_strong_reference(self):
        """Test that postgres_notify_handler stores task in _background_tasks."""

        if not hasattr(cm, '_background_tasks'):  # burn-ignore-introspection
            cm._background_tasks = set()

        async def mock_broadcast(payload):
            pass

        original_broadcast = cm.broadcast_to_redis
        cm.broadcast_to_redis = mock_broadcast
        
        async def run_test():
            postgres_notify_handler(None, None, "test_channel", '{"model": "test"}')
            self.assertTrue(len(cm._background_tasks) > 0, "Task was not added to _background_tasks")
            
            # Wait for task to finish
            await asyncio.gather(*cm._background_tasks)
            self.assertEqual(len(cm._background_tasks), 0, "Task was not removed from _background_tasks")
            
        try:
            asyncio.run(run_test())
        finally:
            cm.broadcast_to_redis = original_broadcast

    def test_cache_manager_db_conns_leak(self):
        """Test that db_conns are closed on exception before clearing."""

        class MockConn:
            def __init__(self):
                self.closed = False
            def is_closed(self):
                return self.closed
            async def close(self):
                self.closed = True
            async def add_listener(self, channel, callback):
                pass
            async def execute(self, q, timeout=None):
                raise Exception("Fake execute error")

        mock_conn = MockConn()

        class FakeRedisClient:
            async def ping(self):
                return True
            async def aclose(self):
                pass

        # Adversarial security review, 2026-09-03: this test used to set up
        # mocks and then never call main() at all -- no assertions, no
        # exercised code path, and a dead mock_reconnect() helper
        # referencing cm.main_db_conns, an attribute that has never existed
        # (the real state, db_conns, is a local variable inside main()).
        # The trailing comment claimed "structural verification is done by
        # direct source code inspection in test_b2_fixes.py" -- confirmed
        # false: that file has no reference to cache_manager at all. This
        # now actually calls main() and drives it through the exact
        # `execute()`-raises path the health check hits, asserting the real
        # documented behavior (connection closed, db_conns cleared) rather
        # than trusting a comment.
        mock_asyncpg = self.safe_patch('odoo.addons.distributed_redis_cache.daemons.cache_manager.asyncpg')

        async def mock_connect(*args, **kwargs):
            return mock_conn
        mock_asyncpg.connect = mock_connect

        redis_patch = self.safe_patch('odoo.addons.distributed_redis_cache.daemons.cache_manager.redis.Redis')
        redis_patch.return_value = FakeRedisClient()

        mock_sleep = self.safe_patch('odoo.addons.distributed_redis_cache.daemons.cache_manager.asyncio.sleep')

        # First sleep() call is the 5s post-exception recovery pause inside
        # the outer except block (line ~205) -- let it proceed normally so
        # the real cleanup code (close + clear) actually runs. The second
        # call is the loop's own 60s idle wait, which would never be
        # reached in a real single-iteration failure -- raise
        # CancelledError there to cleanly stop the daemon after exactly one
        # full failure-and-recovery cycle, the same "stop the while True
        # loop deterministically" intent the original (broken) test had.
        call_count = {"n": 0}

        async def sleep_side_effect(delay):
            call_count["n"] += 1
            if call_count["n"] >= 2:
                raise asyncio.CancelledError()

        mock_sleep.side_effect = sleep_side_effect

        asyncio.run(cm.main())

        self.assertTrue(mock_conn.closed, "the health-check failure's own except block must close the stale connection")

    def test_cache_manager_reconnect_failure_does_not_crash_the_daemon(self):
        # [@ANCHOR: COMM_test_cache_manager_reconnect_failure]
        # Adversarial security review, 2026-09-03: _reconnect()'s own
        # except block (line ~185, "Could not connect to database") was
        # annotated "Tested by COMM_test_cache_manager_exception_handling"
        # but that test only exercises broadcast_to_redis()'s own Redis-
        # publish failure -- confirmed by reading it, this path had no real
        # coverage at all. A daemon that can never reach Postgres in the
        # first place (a real, realistic startup-ordering condition, not
        # just an attack) must log and keep retrying, not crash.
        class FakeRedisClient:
            async def ping(self):
                return True
            async def aclose(self):
                pass

        mock_asyncpg = self.safe_patch('odoo.addons.distributed_redis_cache.daemons.cache_manager.asyncpg')

        async def failing_connect(*args, **kwargs):
            raise ConnectionRefusedError("Fake: Postgres unreachable")
        mock_asyncpg.connect = failing_connect

        redis_patch = self.safe_patch('odoo.addons.distributed_redis_cache.daemons.cache_manager.redis.Redis')
        redis_patch.return_value = FakeRedisClient()

        mock_sleep = self.safe_patch('odoo.addons.distributed_redis_cache.daemons.cache_manager.asyncio.sleep')

        # db_conns starts empty, so main()'s own `if not db_conns:` branch
        # calls _reconnect() on the very first loop iteration, which
        # swallows failing_connect()'s exception internally and leaves
        # db_conns still empty -- the loop then reaches its own
        # `await asyncio.sleep(60)` with no exception ever having reached
        # the outer except block. Raise CancelledError on that first sleep
        # to stop the daemon after exactly one reconnect attempt.
        async def sleep_side_effect(delay):
            raise asyncio.CancelledError()
        mock_sleep.side_effect = sleep_side_effect

        try:
            asyncio.run(cm.main())
        except ConnectionRefusedError:
            self.fail("_reconnect()'s own except block did not swallow the connection failure")
