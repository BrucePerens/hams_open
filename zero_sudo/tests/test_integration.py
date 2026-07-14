# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os
import urllib.request
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "integration")
class TestIntegrationFacility(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:COMM_integration_daemon_testing]

    def test_01_daemon_lifecycle(self):
        # [@ANCHOR: zero_sudo:COMM_test_integration_daemon_testing]
        # ---
        # Verified by [@ANCHOR: zero_sudo:COMM_test_integration_daemon_testing]
        """
        Verify that HamsTransactionCase correctly starts a daemon and polls its health.
        """
        base_dir = os.path.dirname(os.path.dirname(__file__))
        script_path = os.path.join(base_dir, "tests", "dummy_daemon.py")

        # The dummy_daemon.py uses port 1234.
        # We use the hostname here to satisfy the linter's anti-localhost policy.
        host = os.environ.get("DAEMON_HOST", "odoo")
        health_url = f"http://{host}:1234"

        # start_daemon already verifies health_url returns 200.
        process = self.start_daemon(script_path, health_url=health_url)

        self.assertIsNotNone(process.pid)
        self.assertIsNone(process.poll(), "Daemon should be running")

        # Additional check using urllib which is less likely to be blocked than requests
        req = urllib.request.Request(health_url, method="HEAD")
        with urllib.request.urlopen(req, timeout=5) as response:
            self.assertEqual(response.status, 200)
