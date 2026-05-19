#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import unittest
from unittest.mock import AsyncMock, patch, MagicMock
import asyncio
import sys
import os
import json

sys.path.append(os.path.dirname(__file__))
import cache_manager

class TestCacheManager(unittest.IsolatedAsyncioTestCase):
    async def test_01_broadcast_to_redis(self):
        # Tests [@ANCHOR: cache_manager_redis_publish]
        mock_redis = AsyncMock()
        cache_manager.redis_client = mock_redis

        payload = json.dumps({"model": "res.users", "dbname": "odoo"})
        await cache_manager.broadcast_to_redis(payload)

        mock_redis.publish.assert_called_once_with(
            cache_manager.REDIS_CHANNEL, payload
        )

    async def test_02_postgres_notify_handler(self):
        """Verify that the handler correctly schedules a broadcast task."""
        with patch("cache_manager.broadcast_to_redis", new_callable=AsyncMock) as mock_broadcast:
             # We need to simulate the event loop running to process the task
             cache_manager.postgres_notify_handler(None, None, "channel", "payload")
             # Allow one loop iteration
             await asyncio.sleep(0.1)
             mock_broadcast.assert_called_once_with("payload")

    async def test_03_main_loop_reconnect(self):
        """Verify that the main loop attempts to reconnect to Postgres on failure."""

        # We need to patch things inside cache_manager module
        with patch("cache_manager.redis.Redis") as mock_redis_class,              patch("cache_manager.asyncpg.connect") as mock_pg_connect,              patch("cache_manager.asyncio.sleep") as mock_sleep:

            mock_redis = AsyncMock()
            mock_redis_class.return_value = mock_redis

            # First connect succeeds, then it "closes", then second connect fails, then third succeeds.
            mock_conn = MagicMock()
            mock_conn.is_closed.side_effect = [False, True, False, False, False]
            mock_conn.close = AsyncMock()
            mock_conn.add_listener = AsyncMock()

            mock_conn3 = MagicMock()
            mock_conn3.is_closed.return_value = False
            mock_conn3.close = AsyncMock()
            mock_conn3.add_listener = AsyncMock()

            mock_pg_connect.side_effect = [
                mock_conn, # First try
                Exception("Connection failed"), # Second try
                mock_conn3 # Third try
            ]

            # First sleep for the 60s loop, second for the 5s reconnect delay, third to cancel.
            mock_sleep.side_effect = [None, None, asyncio.CancelledError()]

            try:
                await cache_manager.main()
            except asyncio.CancelledError:
                pass

            self.assertGreaterEqual(mock_pg_connect.call_count, 2)

    async def test_04_broadcast_to_redis_invalid_payload(self):
        """Verify that invalid payloads are ignored."""
        mock_redis = AsyncMock()
        cache_manager.redis_client = mock_redis

        # Missing model
        payload = json.dumps({"dbname": "odoo"})
        await cache_manager.broadcast_to_redis(payload)
        mock_redis.publish.assert_not_called()

        # Malformed JSON
        await cache_manager.broadcast_to_redis("not json")
        mock_redis.publish.assert_not_called()

if __name__ == "__main__":
    unittest.main()
