# SPDX-License-Identifier: AGPL-3.0-or-later
import asyncio
from unittest.mock import MagicMock

from odoo.addons.distributed_redis_cache import redis_cache
from odoo.addons.distributed_redis_cache.daemons import cache_manager as cm
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestInvalidateBumpsCounterAfterDelete(HamsTransactionCase):
    # Tests [@ANCHOR: COMM_invalidate_model_cache_logic]
    def test_counter_is_bumped_after_the_keys_are_deleted(self):
        fake = MagicMock()
        fake.scan_iter.return_value = ["db:distributed_cache:m.x:f:w0:c1:abc"]
        self.safe_patch(
            "odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection",
            MagicMock(return_value=fake),
        )
        redis_cache.invalidate_model_cache(self.env, "m.x")
        names = [call[0] for call in fake.method_calls]
        self.assertIn("delete", names)
        self.assertEqual(names[-1], "incr")
        self.assertGreater(names.index("incr"), names.index("delete"))
        self.assertEqual(fake.incr.call_args[0], ("global_cache_invalidation_counter",))

    def test_local_only_invalidation_does_not_touch_the_counter(self):
        fake = MagicMock()
        self.safe_patch(
            "odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection",
            MagicMock(return_value=fake),
        )
        redis_cache.invalidate_model_cache(self.env, "m.x", local_only=True)
        fake.incr.assert_not_called()


@tagged("post_install", "-at_install")
class TestDaemonBumpsCounterOnConnect(HamsTransactionCase):
    # Tests [@ANCHOR: distributed_redis_cache:COMM_cache_manager_bump_counter_on_connect]
    def test_a_successful_connect_bumps_the_counter_so_workers_flush(self):
        incremented = []

        class FakeRedisClient:
            async def ping(self):
                return True

            async def incr(self, key):
                incremented.append(key)
                return 1

            async def aclose(self):
                pass

        class FakeConn:
            async def add_listener(self, channel, callback):
                pass

            def is_closed(self):
                return False

            async def execute(self, query, timeout=None):
                return None

            async def close(self):
                pass

        async def connect(*args, **kwargs):
            return FakeConn()

        async def stop(delay):
            raise asyncio.CancelledError()

        base = "odoo.addons.distributed_redis_cache.daemons.cache_manager."
        self.safe_patch(base + "_require_db_credentials", MagicMock())
        self.safe_patch(base + "asyncpg").connect = connect
        self.safe_patch(base + "redis.Redis").return_value = FakeRedisClient()
        self.safe_patch(base + "asyncio.sleep").side_effect = stop
        asyncio.run(cm.main())
        self.assertEqual(incremented, ["global_cache_invalidation_counter"])
