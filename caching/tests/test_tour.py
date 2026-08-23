# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import os

from odoo.modules.module import get_module_path
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestCachingTour(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.env.ref('base.user_admin').lang = 'en_US'

    def test_caching_service_worker_tour(self):
        """Verify Service Worker registration via tour."""
        self.start_tour("/?debug=1", "caching_service_worker_check", login="admin")

    def test_caching_sw_behavior_tour(self):
        """Verify the SW actually intercepts fetch and populates Cache
        Storage, not just that it registers. See
        docs/proposals/SERVICE_WORKER_TESTING.md."""
        self.start_tour("/?debug=1", "caching_sw_behavior_check", login="admin")

    def test_caching_sw_idb_error_tour(self):
        """Verify openDB() -- the exact function whose missing IndexedDB
        onerror handler used to hang enforceLRUQuota()/updateLRUMetadata()
        forever with no diagnostic -- now settles (rejects) instead of
        hanging when IndexedDB genuinely fails. Forces this via a
        test-only postMessage hook (TEST_FORCE_IDB_ERROR/TEST_CALL_OPEN_DB
        in sw.js) rather than page-context monkeypatching, since the page
        and the SW run in separate global scopes with their own
        `indexedDB`. See docs/proposals/SERVICE_WORKER_TESTING.md's
        "Still open" section, now closed by this test."""
        self.start_tour("/?debug=1", "caching_sw_idb_error_check", login="admin")

    def test_caching_sw_lru_eviction_tour(self):
        """Verify enforceLRUQuota() actually evicts the oldest entries once
        MAX_STORAGE_BYTES is exceeded -- the exact code path a missing
        IndexedDB onerror handler used to make hang forever with no
        diagnostic (see docs/proposals/SERVICE_WORKER_TESTING.md's "Done"
        section). Needs a small quota *before* the tour's first page load,
        since MAX_STORAGE_BYTES is baked into /sw.js at fetch time, not
        read live by the running SW -- a normal ~35MB default quota would
        need tens of real megabytes of seeded data to exceed.

        A flat 1MB quota was tried first and doesn't work: sw.js also
        derives a *per-file* cache-eligibility cap (MAX_FILE_SIZE_BYTES)
        from the same quota via get_global_static_info(), which reserves a
        fixed 10MB floor before computing anything -- any quota_mb <= 10
        makes that cap exactly 0, so the SW's own "SIZE LIMIT SAFETY
        VALVE" refuses to cache *any* non-bundle file at all (the
        triggering fetch this test needs). Above that floor, the cap is
        derived from a real recursive scan of every installed module's
        static/ directory, which this test can't predict from first
        principles -- so ask the model for the real value instead of
        guessing, the same way the model's own get_global_static_info()
        is already tested directly in test_settings_and_cache.py.
        """
        sw_js_path = os.path.join(get_module_path("caching"), "static", "src", "sw", "sw.js")
        sw_js_size = os.path.getsize(sw_js_path)

        caching_mixin = self.env["caching.mixin"].with_context(redis_bypass_cache=True)
        chosen_quota_mb = None
        for candidate_quota_mb in (11, 20, 40, 80, 160, 320):
            _mtime, max_file_size_str = caching_mixin.get_global_static_info(candidate_quota_mb)
            max_file_size = int(max_file_size_str)
            # A generous margin over sw.js's own real size, not just
            # "bigger than" -- the point is the SW must be able to cache
            # sw.js itself without the safety valve tripping.
            if max_file_size > sw_js_size * 4:
                chosen_quota_mb = candidate_quota_mb
                break
        if chosen_quota_mb is None:
            self.fail(
                f"Could not find a quota_mb in (11, 20, 40, 80, 160, 320) whose real, "
                f"filesystem-derived MAX_FILE_SIZE_BYTES comfortably exceeds sw.js's own "
                f"size ({sw_js_size} bytes) -- this environment's installed static assets "
                f"make get_global_static_info()'s per-file cap unexpectedly small at every "
                f"quota tried; the LRU eviction tour can't trigger a real non-bundle cache "
                f"write to test against without one."
            )

        website = self.env["website"].get_current_website()
        website.caching_safe_quota_mb = chosen_quota_mb
        self.start_tour("/?debug=1", "caching_sw_lru_eviction_check", login="admin")
