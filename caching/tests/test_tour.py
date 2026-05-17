# -*- coding: utf-8 -*-
import os
from odoo.tests.common import HttpCase, tagged

@tagged("post_install", "-at_install")
class TestCachingTour(HttpCase):

    def test_caching_service_worker_tour(self):
        # [@ANCHOR: test_caching_service_worker_tour]
        """Verify Service Worker registration via tour."""
        self.start_tour("/?debug=1", "caching_service_worker_check", login="admin")
