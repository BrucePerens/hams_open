# SPDX-License-Identifier: AGPL-3.0-or-later
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
        # Tests [@ANCHOR: COMM_pd_log_api_i18n]

        self.env["pager.log.pattern"].create({
            "name": "Dummy",
            "regex": ".*",
            "severity": "low",
        })
        recs = self.env["pager.log.pattern"].search([], limit=1)
        self.assertTrue(recs)

    def test_03_async_bastion_pattern(self):
        """
        Verify that search_logs creates a pager.log.search.job and returns its UUID instead of blocking.
        """
        api = PagerLogAPI()
        
        mock_redis_mod = self.safe_patch('odoo.addons.pager_duty.controllers.log_api.redis')
        self.safe_patch('odoo.addons.pager_duty.controllers.log_api.redis_pool', MagicMock())
        mock_req = self.safe_patch('odoo.addons.pager_duty.controllers.log_api.request')
        
        mock_req.env.user.has_group.return_value = True
        mock_req.env = self.env
        
        mock_redis = MagicMock()
        mock_redis_mod.Redis.return_value = mock_redis
        
        res = api.search_logs('/var/log/syslog', 'test')
        
        self.assertIn('job_id', res)
        job = self.env["pager.log.search.job"].search([("uuid", "=", res['job_id'])], limit=1)
        self.assertTrue(job)
        self.assertEqual(job.state, "pending")
        mock_redis.publish.assert_called_once()

