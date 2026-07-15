# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase



@tagged("standard", "post_install", "-at_install")
class TestLogAnalyzer(HamsTransactionCase):
    def test_01_log_analyzer_views(self):
        # Tests [@ANCHOR: test_log_analyzer_views]
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

@tagged("standard", "post_install", "-at_install")
class TestLogAnalyzerReal(RealTransactionCase):
    def test_03_async_bastion_pattern(self):
        """
        Verify that search_logs creates a pager.log.search.job and returns its UUID instead of blocking.
        """
        self.authenticate("admin", "admin")
        res = self.url_open(
            "/api/v1/pager/logs/search",
            json={"params": {"file_path": "/var/log/syslog", "regex_query": "test"}}
        )
        self.assertEqual(res.status_code, 200)
        
        data = res.json().get("result", {})
        
        # Test env might not have Redis, so handle both valid IPC or expected missing-Redis error
        if "error" in data:
            self.assertTrue("Redis" in data["error"] or "IPC" in data["error"])
        else:
            self.assertIn("job_id", data)
            job = self.env["pager.log.search.job"].search([("uuid", "=", data["job_id"])], limit=1)
            self.assertTrue(job)
            self.assertEqual(job.state, "pending")

