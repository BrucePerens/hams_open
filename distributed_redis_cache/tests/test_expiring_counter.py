# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
import uuid

import redis

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_pool import expiring_counter, redis_pool


@tagged("post_install", "-at_install")
class TestExpiringCounter(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.client = redis.Redis(connection_pool=redis_pool)
        self.key = f"test:expiring_counter:{uuid.uuid4().hex}"
        self.addCleanup(self.client.delete, self.key)

    def test_01_counts_up_and_keeps_the_first_hits_window(self):
        # Tests [@ANCHOR: distributed_redis_cache:expiring_counter]
        self.assertEqual(expiring_counter(self.client, self.key, 100), 1)
        first_ttl = self.client.ttl(self.key)
        self.assertTrue(0 < first_ttl <= 100)
        self.assertEqual(expiring_counter(self.client, self.key, 5000), 2)
        self.assertEqual(expiring_counter(self.client, self.key, 5000), 3)
        self.assertTrue(0 < self.client.ttl(self.key) <= 100, "a later hit must not extend or replace the window")

    def test_02_a_failing_expire_cannot_leave_a_key_without_an_expiry(self):
        # Tests [@ANCHOR: distributed_redis_cache:expiring_counter]
        expire = self.safe_patch_object(redis.Redis, "expire", side_effect=redis.RedisError("simulated failure between INCR and EXPIRE"))
        self.assertEqual(expiring_counter(self.client, self.key, 100), 1)
        expire.assert_not_called()  # the expiry is set in the same transaction as the first hit
        self.assertTrue(0 < self.client.ttl(self.key) <= 100)

    def test_03_a_key_the_old_pattern_left_without_an_expiry_heals(self):
        # Tests [@ANCHOR: distributed_redis_cache:expiring_counter]
        self.client.set(self.key, 50)
        self.assertEqual(self.client.ttl(self.key), -1)
        self.assertEqual(expiring_counter(self.client, self.key, 100), 51)
        self.assertTrue(0 < self.client.ttl(self.key) <= 100)

    def test_04_a_redis_error_reaches_the_caller(self):
        # Tests [@ANCHOR: distributed_redis_cache:expiring_counter]
        self.client.set(self.key, 50)
        self.safe_patch_object(redis.Redis, "expire", side_effect=redis.RedisError("simulated"))
        with self.assertRaises(redis.RedisError):
            expiring_counter(self.client, self.key, 100)

    def test_05_an_amount_is_added_in_one_step_and_the_key_expires(self):
        # Tests [@ANCHOR: distributed_redis_cache:expiring_counter]
        # A caller that counts weighted hits (the callbook search adds the number of rows it returned) passes `amount`.
        expire = self.safe_patch_object(redis.Redis, "expire", side_effect=redis.RedisError("simulated"))
        self.assertEqual(expiring_counter(self.client, self.key, 100, amount=7), 7)
        self.assertEqual(expiring_counter(self.client, self.key, 100, amount=5), 12)
        expire.assert_not_called()
        self.assertTrue(0 < self.client.ttl(self.key) <= 100)
