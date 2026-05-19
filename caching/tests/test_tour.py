# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsHttpCase

@tagged("post_install", "-at_install")
class TestCachingTour(HamsHttpCase):

    def test_caching_service_worker_tour(self):
        # [@ANCHOR: test_caching_service_worker_tour]
        """Verify Service Worker registration via tour."""
        self.start_tour("/?debug=1", "caching_service_worker_check", login="admin")
