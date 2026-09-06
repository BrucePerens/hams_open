# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

import re
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestZeroSudoControllers(RealTransactionCase):

    def test_01_web_login_interceptor(self):
        # [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor_check]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor_check]
        # ---
        # [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_web_login_interceptor]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_web_login_interceptor_check]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_zero_sudo_security_log_global]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_url_open]
        """Verify that service accounts cannot log into the web interface."""
        # [@ANCHOR: zero_sudo:COMM_test_is_service_account_field]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_is_service_account_field]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_is_service_account_field]

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
                "lang": "en_US",
            }
        )
        self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor] # fmt: skip
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
            data={  # burn-ignore-route # fmt: skip
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
        # RealTransactionCase: the log entry was created by the HTTP
        # worker's own connection -- commit() (not just invalidate_all(),
        # which only clears the ORM cache) is what lets this cursor see
        # it in a fresh transaction.
        self.env.cr.commit()
        self.env.invalidate_all()
        log_entry = self.env["zero_sudo.security.log"].search(
            [("user_id", "=", user.id), ("reason", "=", "service_account_blocked")],
            limit=1,
        )
        self.assertTrue(
            log_entry,
            msg="[!] DIAGNOSTIC FOR AI: Security log entry was not created for blocked login.",
        )

    def test_02_service_account_session_blocked_from_interactive_web_ui(self):
        # Tests [@ANCHOR: zero_sudo:ir_http_authenticate]
        """The /web/login form interceptor above blocks a service account
        from ever obtaining a session that way -- but _authenticate() is
        the deeper, request-dispatch-level guard for a service account
        that already HAS a session by some other means (this test forges
        one directly via self.authenticate(), the same way a stolen or
        otherwise-issued session cookie would), catching any real request
        to an ordinary web page, not just the login form itself."""
        login = "test_service_session_block"
        password = "test_password"
        user = self.env["res.users"].create(
            {
                "name": "Test Service Session Block",
                "login": login,
                "password": password,
                "is_service_account": False,
                "active": True,
                "lang": "en_US",
            }
        )
        self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:ir_http_authenticate] # fmt: skip
            "UPDATE res_users SET is_service_account = True WHERE id = %s", (user.id,)
        )
        self.env.cr.commit()

        self.authenticate(login, password)
        response = self.url_open("/odoo", allow_redirects=False)
        # AccessError raised from _authenticate() is mapped to a real 403
        # Forbidden by Odoo's own request-dispatch error handling (not the
        # generic 500 an uncaught exception elsewhere would produce).
        self.assertEqual(
            response.status_code,
            403,
            "[!] DIAGNOSTIC FOR AI: a service account session hitting an "
            "ordinary web route must be rejected by _authenticate()'s own "
            "AccessError, not served like a normal user's session.",
        )
