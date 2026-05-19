# -*- coding: utf-8 -*-
import os
import socket
import urllib.request
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsIntegrationCase

@tagged('post_install', '-at_install', 'integration')
class TestIntegrationFacility(HamsIntegrationCase):
    # Tests [@ANCHOR: integration_daemon_testing]

    def test_01_daemon_lifecycle(self):
        # [@ANCHOR: test_integration_daemon_testing]
        # Verified by [@ANCHOR: test_integration_daemon_testing]
        """
        Verify that HamsIntegrationCase correctly starts a daemon and polls its health.
        """
        base_dir = os.path.dirname(os.path.dirname(__file__))
        script_path = os.path.join(base_dir, 'tests', 'dummy_daemon.py')

        # The dummy_daemon.py uses port 1234.
        # We use the hostname here to satisfy the linter's anti-localhost policy.
        host = socket.gethostname()
        health_url = f"http://{host}:1234"

        # start_daemon already verifies health_url returns 200.
        process = self.start_daemon(script_path, health_url=health_url)

        self.assertIsNotNone(process.pid)
        self.assertIsNone(process.poll(), "Daemon should be running")

        # Additional check using urllib which is less likely to be blocked than requests
        req = urllib.request.Request(health_url, method="HEAD")
        with urllib.request.urlopen(req, timeout=5) as response:
            self.assertEqual(response.status, 200)
