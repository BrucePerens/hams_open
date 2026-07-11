# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import re
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestZeroSudoControllers(RealTransactionCase):

    def test_01_web_login_interceptor(self):
        # [@ANCHOR: test_web_login_interceptor_check]
        # [@ANCHOR: test_web_login_interceptor]
        # Tests [@ANCHOR: web_login_interceptor]
        # Tests [@ANCHOR: web_login_interceptor_check]
        # Tests [@ANCHOR: story_login_blocking]
        # Tests [@ANCHOR: journey_service_account_lifecycle]
        # Tests [@ANCHOR: zero_sudo_security_log_global]
        """Verify that service accounts cannot log into the web interface."""
        # [@ANCHOR: test_is_service_account_field]
        # Tests [@ANCHOR: is_service_account_field]

        login = "test_service_block"
        password = "test_password"

        # 1. Create a service account
        # We create it as a human first, then toggle to Service Account
        # to bypass the automatic password randomization in create()
        # so we can actually test the controller-level blocking.
        user = self.env["res.users"].create(
            {
                "name": "Test Service Block",
                "login": login,
                "password": password,
                "is_service_account": False,
                "active": True,
            }
        )
        self.env.cr.execute(  # audit-ignore-sql
            "UPDATE res_users SET is_service_account = True WHERE id = %s", (user.id,)
        )
        # MANDATORY: Commit so the HTTP worker thread can see the new user.
        # RealTransactionCase will handle the cleanup in tearDown.
        self.env.cr.commit()

        # 2. Attempt login via POST to /web/login
        # We fetch the login page first to get a session and CSRF token
        response = self.url_open("/web/login")  # burn-ignore-route
        csrf_token = ""

        match = re.search(r'name="csrf_token"\s+value="([^"]+)"', response.text)
        if match:
            csrf_token = match.group(1)

        response = self.url_open(
            "/web/login",
            data={  # burn-ignore-route
                "login": login,
                "password": password,
                "csrf_token": csrf_token,
            },
            allow_redirects=False,
        )

        # 3. Check if we were redirected to login with error
        # A 303 redirect means the interceptor triggered and called request.redirect
        self.assertEqual(
            response.status_code,
            303,
            msg="[!] DIAGNOSTIC FOR AI: Expected 303 redirect from login interceptor.",
        )
        self.assertIn(
            "error=access_denied_service",
            response.headers.get("Location", ""),
            msg="[!] DIAGNOSTIC FOR AI: Expected error parameter 'access_denied_service' missing in redirect Location header.",
        )

        # 4. Verify security log entry
        self.env.invalidate_all()
        log_entry = self.env["zero_sudo.security.log"].search(
            [("user_id", "=", user.id), ("reason", "=", "service_account_blocked")]
        )
        self.assertTrue(
            log_entry,
            msg="[!] DIAGNOSTIC FOR AI: Security log entry was not created for blocked login.",
        )
