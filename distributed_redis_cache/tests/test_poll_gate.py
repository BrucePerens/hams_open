# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Regression coverage for the cache-invalidation poll gate shared by both poll sites.

Both cache-invalidation poll sites -- `ir.http._authenticate` (COMM_redis_cache_interceptor) and
`ir.cron._process_job` (COMM_cron_cache_interceptor) -- used to skip the poll whenever any of
`tools.config`'s `init`, `update` or `stop_after_init` was set, meaning to skip Redis during module
install and upgrade. Odoo 19 never clears those flags afterwards, so an Odoo started as
`odoo -u some_module` that then went on to serve kept `update` truthy for its whole lifetime, and
every HTTP worker and cron worker in it served its process-local L1 cache forever without ever
reading `global_cache_invalidation_counter`. These tests pin the replacement gate, which asks the
registry whether it is still loading instead.
"""
# Tests [@ANCHOR: COMM_should_poll_for_invalidation]

# Tests [@ANCHOR: COMM_redis_cache_interceptor]

from odoo import tools
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase, HamsHttpCase
from odoo.addons.distributed_redis_cache.redis_cache import should_poll_for_invalidation


class _StubRegistry:
    """Stands in for a registry that is still loading.

    The live registry a test runs on is genuinely ready and must not be told otherwise: Odoo reads
    `Registry.ready` for its own dispatch and invalidation decisions (`signal_changes()` logs a
    warning and declines outright when it is False), so flipping it under a running test would
    exercise Odoo's broken-state handling rather than this gate.
    """

    def __init__(self, ready):
        self.ready = ready


@tagged("post_install", "-at_install")
class TestPollGate(HamsTransactionCase):

    def test_a_ready_registry_polls_even_though_update_is_set(self):
        """The actual regression, and it needs no patching to reproduce.

        This very process was started with an Odoo loading flag on its command line, that flag is
        STILL set now that loading has finished, and the registry is ready and serving tests --
        which is the whole bug in one place. A server started with `-i`/`-u` and left serving is in
        exactly this state, and the old gate refused to poll for its entire lifetime, so its
        workers never invalidated their L1 cache again.

        The premise is asserted rather than argued from source reading, so that if Odoo ever does
        start clearing these flags, this test says so instead of quietly passing.
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

        self.assertTrue(
            should_poll_for_invalidation(self.registry),
            "a ready registry must poll, even though `update` is still set from the command line "
            "that started the process",
        )

    def test_a_loading_registry_never_polls(self):
        """Module install and upgrade must not depend on Redis. This is what the old
        `init`/`update`/`stop_after_init` gate was reaching for, stated accurately."""
        self.assertFalse(
            should_poll_for_invalidation(_StubRegistry(ready=False)),
            "a registry that is still being built must not make the process depend on Redis",
        )

    def test_no_registry_never_polls(self):
        """Defensive: a call site with no registry to ask cannot know that polling is safe."""
        self.assertFalse(should_poll_for_invalidation(None))


@tagged("post_install", "-at_install")
class TestHttpPollGateCallSite(HamsHttpCase):
    """The HTTP half of the pair. `ir.cron._process_job`'s call site is covered by
    tests/test_cron_cache_interceptor.py; this covers `ir.http._authenticate`, driven by a real
    request through the real request lifecycle rather than by calling the override directly."""

    def _spy_on_the_poll(self):
        calls = []
        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_http.poll_and_clear_local_cache",
            side_effect=lambda env: calls.append(env),
        )
        return calls

    def test_a_real_request_polls_with_the_real_gate(self):
        """End to end, nothing about the gate patched: a served request on a ready registry polls.

        This is the HTTP counterpart of test_a_ready_registry_polls_even_though_update_is_set --
        this process has `update` set, and under the old gate this request would not have polled.
        """
        calls = self._spy_on_the_poll()

        response = self.url_open("/web/login")
        self.assertEqual(response.status_code, 200)

        self.assertTrue(
            calls,
            "a served request on a ready registry must poll for cache invalidation",
        )

    def test_a_real_request_does_not_poll_when_the_gate_refuses(self):
        """The call site must honour a refusal rather than polling unconditionally -- this is the
        install/upgrade protection, reached through the real request path."""
        calls = self._spy_on_the_poll()
        seen = []

        def refuse(registry):
            seen.append(registry)
            return False

        self.safe_patch(
            "odoo.addons.distributed_redis_cache.models.ir_http.should_poll_for_invalidation",
            side_effect=refuse,
        )

        response = self.url_open("/web/login")
        self.assertEqual(response.status_code, 200)

        self.assertTrue(seen, "the gate must actually be consulted on the request path")
        self.assertEqual(
            seen, [self.env.registry] * len(seen),
            "the gate must be consulted with this request's own registry (`cls.pool`)",
        )
        self.assertEqual(calls, [], "a refused gate must keep the request path off Redis")
