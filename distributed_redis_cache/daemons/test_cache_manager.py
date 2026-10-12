#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-

import os
import sys
import unittest
from unittest.mock import patch, AsyncMock

# Bridge the local directory to import the daemon module for testing,
# matching daemons/dx_firehose/test_dx_firehose.py's own convention.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cache_manager  # noqa: E402


class RequireDbCredentialsTests(unittest.IsolatedAsyncioTestCase):
    """Tests [@ANCHOR: COMM_cache_manager_require_db_credentials]

    night_shift_todo/low/cache-manager-odoo-password-fallback-0ea55461.md: DB_USER/DB_PASS used to
    default to the literal "odoo"/"odoo" whenever DB_ENV_FILE was absent -- the normal case on
    every deployment before infrastructure.py's own provisioning learned to write it (see
    infrastructure.py's _provision_cache_manager_role). An unset DB_USER/DB_PASS pair must now
    stop main() before it ever reaches Redis or PostgreSQL, not silently connect as a
    full-privilege role with a guessable literal password.
    """

    def test_refuses_to_start_without_both_credentials(self):
        for user, password in (("cache_manager_ro", ""), ("", "real-password"), ("", "")):
            with self.subTest(user=user, password=password), \
                 patch.object(cache_manager, "DB_USER", user), \
                 patch.object(cache_manager, "DB_PASS", password):
                with self.assertRaises(RuntimeError):
                    cache_manager._require_db_credentials()

    def test_does_not_raise_when_both_credentials_are_present(self):
        with patch.object(cache_manager, "DB_USER", "cache_manager_ro"), \
             patch.object(cache_manager, "DB_PASS", "real-password"):
            cache_manager._require_db_credentials()  # must not raise

    async def test_main_refuses_to_start_without_a_database_password(self):
        # Tests [@ANCHOR: COMM_cache_manager_require_db_credentials]
        # Proves the guard is actually wired into main(), not just defined
        # and never called -- main() must raise before it ever touches
        # Redis (create_redis_client below would otherwise be a real
        # connection attempt against whatever REDIS_HOST happens to
        # resolve to in this test environment).
        create_redis = self.safe_patch("cache_manager.redis.Redis", new=AsyncMock())
        with patch.object(cache_manager, "DB_USER", ""), \
             patch.object(cache_manager, "DB_PASS", ""):
            with self.assertRaises(RuntimeError):
                await cache_manager.main()
        create_redis.assert_not_called()

    def safe_patch(self, target, **kwargs):
        patcher = patch(target, **kwargs)
        mock_obj = patcher.start()
        self.addCleanup(patcher.stop)
        return mock_obj


class UnitFileTests(unittest.TestCase):
    """The shipped cache-manager.service must be one that works on a real host: found on hams1,
    2026-10-12, the sample unit pointed at /opt/odoo/addons and loaded no Redis credentials, and no
    deployment installed it, so the global_cache_invalidation_counter never existed."""

    def setUp(self):
        path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cache-manager.service")
        with open(path, encoding="utf-8") as f:
            self.text = f.read()

    def test_unit_loads_the_redis_login(self):
        self.assertIn("EnvironmentFile=/opt/hams/etc/redis.env\n", self.text)

    def test_unit_runs_the_deployed_file_that_exists(self):
        self.assertNotIn("/opt/odoo/addons", self.text)
        self.assertIn(
            "ExecStart=/usr/bin/python3 /opt/hams/src/hams_open/distributed_redis_cache/daemons/cache_manager.py\n",
            self.text,
        )
        self.assertTrue(os.path.isfile(os.path.join(os.path.dirname(os.path.abspath(__file__)), "cache_manager.py")))


if __name__ == "__main__":
    unittest.main()
