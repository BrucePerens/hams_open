#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import unittest
from unittest.mock import AsyncMock
from odoo.tests.common import tagged
import cache_manager


@tagged("standard", "post_install", "-at_install")
class TestCacheManager(unittest.IsolatedAsyncioTestCase):
    async def test_01_broadcast_to_redis(self):
        # Tests [@ANCHOR: cache_manager_redis_publish]
        mock_redis = AsyncMock()
        cache_manager.redis_client = mock_redis

        await cache_manager.broadcast_to_redis("test_payload")

        mock_redis.publish.assert_called_once_with(
            cache_manager.REDIS_CHANNEL, "test_payload"
        )


if __name__ == "__main__":
    unittest.main()
