# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import os
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("standard", "post_install", "-at_install")
class TestJourneysStories(HamsTransactionCase):
    def test_journeys_realized(self):
        # Simply verify that the journey files exist in the module
        base_path = os.path.join(os.path.dirname(__file__), "..", "docs", "journeys")
        expected_files = [
            "daemon_execution_loop.md",
            "escalation_pathway.md",
            "incident_lifecycle.md",
            "synthetic_monitoring_flow.md",
        ]
        for f in expected_files:
            self.assertTrue(
                os.path.exists(os.path.join(base_path, f)),
                "Journey file %s missing" % f,
            )

    def test_stories_realized(self):
        # Simply verify that the story files exist in the module
        base_path = os.path.join(os.path.dirname(__file__), "..", "docs", "stories")
        expected_files = [
            "automated_monitoring_setup.md",
            "log_anomaly_detection.md",
            "on_call_alerting.md",
            "performance_analytics.md",
        ]
        for f in expected_files:
            self.assertTrue(
                os.path.exists(os.path.join(base_path, f)), "Story file %s missing" % f
            )
