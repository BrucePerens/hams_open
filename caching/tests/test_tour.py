# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
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
