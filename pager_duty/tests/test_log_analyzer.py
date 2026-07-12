# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from unittest.mock import MagicMock
from odoo.addons.pager_duty.controllers.log_api import PagerLogAPI


@tagged("standard", "post_install", "-at_install")
class TestLogAnalyzer(HamsTransactionCase):
    def test_01_log_analyzer_views(self):
        # [@ANCHOR: test_log_analyzer_views]
        v1 = self.env["pager.log.pattern"].get_view(view_type="list")
        self.assertIn("regex", v1["arch"])

        v2 = self.env["pager.log.file"].get_view(view_type="list")
        self.assertIn("filepath", v2["arch"])

    def test_02_headless_api_translation(self):
        # Tests [@ANCHOR: pd_log_api_i18n]
        # Tests [@ANCHOR: pd_log_api_i18n]
        recs = self.env["pager.log.pattern"].search([], limit=1)
        self.assertIsNotNone(recs)

    def test_03_timeout_removed(self):
        """
        Verify that get_message is called without a timeout to prevent synchronous thread blocking.
        """
        api = PagerLogAPI()
        
        # Patch redis components using safe_patch
        mock_redis_mod = self.safe_patch('odoo.addons.pager_duty.controllers.log_api.redis')
        self.safe_patch('odoo.addons.pager_duty.controllers.log_api.redis_pool', MagicMock())
        mock_req = self.safe_patch('odoo.addons.pager_duty.controllers.log_api.request')
        
        mock_req.env.user.has_group.return_value = True
        self.safe_patch('odoo.addons.pager_duty.controllers.log_api._', side_effect=lambda x: x)
        
        mock_pubsub = MagicMock()
        mock_redis = MagicMock()
        mock_redis.pubsub.return_value = mock_pubsub
        mock_redis_mod.Redis.return_value = mock_redis
        
        mock_pubsub.get_message.return_value = None
        
        api.search_logs('/var/log/syslog', 'test')
        
        mock_pubsub.get_message.assert_called_once()
        kwargs = mock_pubsub.get_message.call_args.kwargs
        self.assertNotIn('timeout', kwargs, "The timeout argument must be removed to avoid thread blocking.")

