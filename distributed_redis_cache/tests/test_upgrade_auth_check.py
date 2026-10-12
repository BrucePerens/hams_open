# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""A module install/upgrade whose Redis credentials are rejected must stop at the first cache invalidation.

Found 2026-10-12: a hand-run `odoo -u` on production lacked the service's Redis credentials, 2394 cache
invalidations failed as WARNINGs and the upgrade "succeeded" with every pre-upgrade entry still in Redis.
"""
# Tests [@ANCHOR: distributed_redis_cache:COMM_invalidation_auth_failure_aborts_upgrade]

import redis

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache import redis_cache


class _Registry:
    def __init__(self, ready):
        self.ready = ready


class _Cr:
    dbname = "auth_probe_db"


class _Env:
    cr = _Cr()

    def __init__(self, ready):
        self.registry = _Registry(ready)


class _Client:
    def __init__(self, exc):
        self.exc = exc

    def scan_iter(self, **kwargs):
        raise self.exc


@tagged("post_install", "-at_install")
class TestUpgradeAuthCheck(HamsTransactionCase):

    def _invalidate(self, ready, exc):
        self.safe_patch_object(redis_cache, "get_redis_connection", return_value=_Client(exc))
        redis_cache.invalidate_model_cache(_Env(ready), "res.partner")

    def test_rejected_credentials_while_the_registry_is_loading_abort_the_upgrade(self):
        with self.assertRaisesRegex(RuntimeError, "odoo_upgrade.py"):
            self._invalidate(False, redis.AuthenticationError("Authentication required."))

    def test_a_wrong_password_aborts_it_too(self):
        with self.assertRaises(RuntimeError):
            self._invalidate(False, redis.AuthenticationError("invalid username-password pair"))

    def test_a_serving_worker_only_logs_a_warning(self):
        with self.assertLogs(redis_cache._logger, "WARNING") as logs:
            self._invalidate(True, redis.AuthenticationError("Authentication required."))
        self.assertIn("Redis cache invalidation failed", logs.output[0])

    def test_an_unreachable_redis_never_fails_an_upgrade(self):
        with self.assertLogs(redis_cache._logger, "WARNING"):
            self._invalidate(False, redis.ConnectionError("refused"))
