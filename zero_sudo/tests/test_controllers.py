# -*- coding: utf-8 -*-
import re
from odoo.tests.common import HttpCase, tagged

@tagged("post_install", "-at_install")
class TestZeroSudoControllers(HttpCase):

    def test_01_web_login_interceptor(self):
        # [@ANCHOR: test_web_login_interceptor]
        # Tests [@ANCHOR: web_login_interceptor]
        # Tests [@ANCHOR: web_login_interceptor_check]
        # Tests [@ANCHOR: story_login_blocking]
        # Tests [@ANCHOR: journey_service_account_lifecycle]
        """Verify that service accounts cannot log into the web interface."""
        # [@ANCHOR: test_is_service_account_field]
        # Tests [@ANCHOR: is_service_account_field]

        login = 'test_service_block'
        password = 'test_password'

        # 1. Create a service account
        self.env['res.users'].create({
            'name': 'Test Service Block',
            'login': login,
            'password': password,
            'is_service_account': True,
        })

        # 2. Attempt login via POST to /odoo/login
        # We fetch the login page first to get a session and CSRF token
        response = self.url_open('/odoo/login')
        csrf_token = ''

        match = re.search(r'name="csrf_token"\s+value="([^"]+)"', response.text)
        if match:
            csrf_token = match.group(1)

        response = self.url_open('/odoo/login', data={
            'login': login,
            'password': password,
            'csrf_token': csrf_token,
        })

        # 3. Check if we were redirected to login with error
        self.assertIn('/odoo/login', response.url)
        self.assertIn('error=access_denied_service', response.url)
