# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Test helper: a real Redis client with one command made to fail, shared by the tests of every module that keeps a rate-limit counter."""
import redis


# [@ANCHOR: distributed_redis_cache:test_expire_fails_client]
class ExpireFailsClient:
    """Wraps a real `redis.Redis`; every command goes through to it except EXPIRE, which raises `redis.RedisError`
    (and counts the call in `expire_calls`). It is the failure between INCR and EXPIRE that used to leave a
    rate-limit key without an expiry. `redis.Redis` is replaced by `lambda connection_pool=None: ExpireFailsClient(real)`
    in a caller's module with `safe_patch`."""

    def __init__(self, client):
        self._client = client
        self.expire_calls = 0

    def expire(self, key, seconds):
        self.expire_calls += 1
        raise redis.RedisError("simulated failure between INCR and EXPIRE")

    def __getattr__(self, name):
        return getattr(self._client, name)
