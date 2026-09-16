# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Regression coverage for `ir.cron._process_job`'s cache-invalidation poll.

`ir.http._authenticate`'s poll-and-clear (COMM_redis_cache_interceptor) only ever runs on the HTTP
request path. `odoo/service/server.py`'s cron dispatch -- both the single-process `cron%d` thread
and the real multi-worker `WorkerCron.process_work()` -- calls `IrCron._process_jobs(db_name)` by
importing `ir.cron`'s base module class directly, which never reaches any `_inherit`-based override.
`_process_jobs_loop` (shared by both dispatch paths) is the one place that routes back through the
registry, specifically to reach per-job overrides -- these tests exercise that exact call, the way
real cron dispatch does, rather than calling the new override function directly.
"""
import secrets

# Tests [@ANCHOR: COMM_cron_cache_interceptor]

# Tests [@ANCHOR: COMM_should_poll_for_invalidation]

from odoo import fields, tools
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
        """The override must run the poll on every real per-job dispatch, not just on paper.

        Nothing about the gate is patched here. This process still carries Odoo's loading flags
        from the command line that started it, and its registry is ready anyway -- the exact
        production shape the old `init`/`update`/`stop_after_init` gate skipped for a process's
        whole lifetime. COMM_should_poll_for_invalidation asks the registry instead, so the poll
        runs.
        """
        # The premise the whole fix rests on, asserted here rather than argued from source
        # reading: Odoo's loading flags are still set in this process, and the registry is
        # nevertheless ready. That is exactly the production shape the old gate refused on, and
        # this process is in it. On this box the still-set flags are `init` (hams_shared/tools/
        # test.py invokes odoo with `-i`, not `-u`) and `stop_after_init` (forced by
        # `--test-enable`); asserting "at least one" rather than naming one keeps this honest if
        # that invocation changes.
        still_set = {
            flag: tools.config.get(flag)
            for flag in ("init", "update", "stop_after_init")
            if tools.config.get(flag)
        }
        self.assertTrue(
            still_set,
            "premise: at least one of Odoo's loading flags is still set after loading finished -- "
            "if this ever fails, Odoo started clearing them and the bug this gate fixes is gone",
        )
        self.assertTrue(
            self.registry.ready,
            "premise: post_install suites are built from an already-loaded registry, so the flags "
            f"above are set on a registry that IS serving. Still set: {still_set}",
        )
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

    def test_cron_dispatch_skips_the_poll_when_the_gate_refuses(self):
        """Replaces test_cron_dispatch_skips_the_poll_during_stop_after_init.

        The install/upgrade protection that old test covered now lives in
        COMM_should_poll_for_invalidation, which refuses whenever the registry is still being
        built -- a failed upgrade because Redis was unreachable would be a real regression, not
        a safety improvement. The old `init`/`update`/`stop_after_init` form of that protection
        is what broke: Odoo 19 never clears those flags once loading finishes, so a serving `-u`
        process was held back forever.

        The gate's own verdict for a loading registry is asserted against a stub in
        tests/test_poll_gate.py. What belongs here is the other half: that this call site
        actually consults the gate, passes its own registry, and honours a refusal."""
        calls = []
        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_cron.poll_and_clear_local_cache",
            side_effect=lambda env: calls.append(env),
        )
        # The gate's own answer for a loading registry is asserted directly, against a stub, in
        # tests/test_poll_gate.py -- the live registry this test runs on is genuinely ready and
        # must not be told otherwise, since Odoo reads `ready` for its own dispatch decisions.
        # What this test covers is the call site: `_process_job` must honour a refusal.
        seen = []

        def refuse(registry):
            seen.append(registry)
            return False

        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_cron.should_poll_for_invalidation",
            side_effect=refuse,
        )

        with self.enter_registry_test_mode(), self.registry.cursor() as cr:
            cron, job = self._acquire_real_job(cr)
            self.registry["ir.cron"]._process_job(cr, job)

        self.assertEqual(calls, [], "must not poll Redis while the registry is still loading")
        self.assertEqual(
            seen, [self.registry],
            "the gate must be consulted with this cron worker's own registry (`cls.pool`)",
        )
