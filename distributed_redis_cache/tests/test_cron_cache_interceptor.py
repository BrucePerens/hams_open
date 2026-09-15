# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Regression coverage for [@ANCHOR: distributed_redis_cache:COMM_cron_cache_interceptor].

`ir.http._authenticate`'s poll-and-clear (COMM_redis_cache_interceptor) only ever runs on the HTTP
request path. `odoo/service/server.py`'s cron dispatch -- both the single-process `cron%d` thread
and the real multi-worker `WorkerCron.process_work()` -- calls `IrCron._process_jobs(db_name)` by
importing `ir.cron`'s base module class directly, which never reaches any `_inherit`-based override.
`_process_jobs_loop` (shared by both dispatch paths) is the one place that routes back through the
registry, specifically to reach per-job overrides -- these tests exercise that exact call, the way
real cron dispatch does, rather than calling the new override function directly.
"""
import secrets

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import _local_cache, LRU_LOCK
import odoo.addons.distributed_redis_cache.redis_cache as redis_cache


def _dummy_cron_vals(env):
    return {
        "name": f"Dummy cron for TestCronCacheInterceptor {secrets.token_urlsafe(8)}",
        "state": "code",
        "code": "",
        "model_id": env.ref("base.model_res_partner").id,
        "user_id": env.uid,
        "active": True,
        "interval_number": 1,
        "interval_type": "days",
        "numbercall": 1,
    }


@tagged("post_install", "-at_install")
class TestCronCacheInterceptor(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        # Isolate from whatever other tests left in the shared module-level
        # cache/counter -- both are process-global state by design.
        with LRU_LOCK:
            _local_cache.clear()
        redis_cache._last_cache_counter = None
        self.addCleanup(redis_cache._local_cache.clear)

    def _acquire_real_job(self, cr):
        """Create a real, harmless (empty server-action) cron and acquire it as a real job dict,
        the same way `_process_jobs_loop` does before calling `_process_job`."""
        cron = self.env["ir.cron"].create(_dummy_cron_vals(self.env))
        cron.flush_recordset()
        job = self.env["ir.cron"]._acquire_one_job(cr, cron.id)
        self.assertIsNotNone(job, "setup bug: could not acquire the job we just created")
        return cron, job

    def test_cron_dispatch_polls_before_running_the_job(self):
        """The override must run the poll on every real per-job dispatch, not just on paper."""
        calls = []
        orig_poll = redis_cache.poll_and_clear_local_cache

        def spy(env):
            calls.append(env)
            return orig_poll(env)

        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_cron.poll_and_clear_local_cache",
            side_effect=spy,
        )
        # Real Redis is not assumed reachable in this environment -- a RedisError is caught and
        # logged by poll_and_clear_local_cache itself, so the spy still records the call either way.

        with self.enter_registry_test_mode(), self.registry.cursor() as cr:
            cron, job = self._acquire_real_job(cr)
            self.registry["ir.cron"]._process_job(cr, job)

        self.assertEqual(len(calls), 1, "the cron dispatch path must poll exactly once per job")

    def test_cron_dispatch_actually_clears_a_stale_l1_entry(self):
        """End-to-end: a changed Redis counter must clear _local_cache via the cron path alone,
        with no HTTP request (_authenticate) ever involved -- this is the exact production gap
        (a WorkerCron-only process, e.g. the cloudflare purge-queue cron) this override closes."""

        class FakeRedis:
            def get(self, key):
                assert key == "global_cache_invalidation_counter"
                return "some-new-counter-value"

        self.safe_patch(
            "odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection",
            return_value=FakeRedis(),
        )

        with LRU_LOCK:
            _local_cache["some:stale:key"] = "stale value"

        with self.enter_registry_test_mode(), self.registry.cursor() as cr:
            cron, job = self._acquire_real_job(cr)
            self.registry["ir.cron"]._process_job(cr, job)

        self.assertNotIn(
            "some:stale:key", _local_cache,
            "a changed invalidation counter must clear the L1 cache via the cron dispatch path",
        )

    def test_cron_dispatch_skips_the_poll_during_stop_after_init(self):
        """Matches ir.http._authenticate's own skip during init/update/stop_after_init -- a Redis
        dependency during module install/upgrade/shutdown-after-init would be a real regression,
        not a safety improvement."""
        calls = []
        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_cron.poll_and_clear_local_cache",
            side_effect=lambda env: calls.append(env),
        )
        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_cron.tools.config.get",
            side_effect=lambda key, default=None: True if key == "stop_after_init" else default,
        )

        with self.enter_registry_test_mode(), self.registry.cursor() as cr:
            cron, job = self._acquire_real_job(cr)
            self.registry["ir.cron"]._process_job(cr, job)

        self.assertEqual(calls, [], "must not poll Redis while stop_after_init is set")
