# -*- coding: utf-8 -*-
import uuid

import odoo.http
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase


@tagged("post_install", "-at_install")
class TestUserLockoutServiceAccount(HamsTransactionCase):

    def test_service_account_is_least_privilege(self):
        """Verify the account-lockout service account is correctly
        configured and can't be silently widened into a privilege
        escalation -- it must resolve via the zero-sudo lookup, be a
        real service account, and hold neither of the two groups the
        zero_sudo_get_service_uid() Postgres procedure itself treats as
        disqualifying (see zero_sudo/data/postgres_procedures.xml)."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.user_lockout_service_internal"
        )
        svc_user = self.env["res.users"].browse(svc_uid)
        msg1 = (
            "[!] DIAGNOSTIC FOR AI: Service account 'user_lockout_service_internal' should be active. "
            "Check zero_sudo/data/security_data.xml."
        )
        self.assertTrue(svc_user.active, msg1)
        msg2 = (
            "[!] DIAGNOSTIC FOR AI: User should be marked as a service account (is_service_account=True). "
            "Check zero_sudo/data/security_data.xml."
        )
        self.assertTrue(svc_user.is_service_account, msg2)

        escalation_msg = (
            "[!] DIAGNOSTIC FOR AI: The account-lockout service account must never hold "
            "base.group_system or base.group_erp_manager -- that would violate the zero-sudo "
            "mandate (see zero_sudo_get_service_uid()'s own privilege-escalation check) and "
            "give it far more power than the one res.users write it actually needs."
        )
        self.assertFalse(
            svc_user.has_group("base.group_system"), escalation_msg
        )
        self.assertFalse(
            svc_user.has_group("base.group_erp_manager"), escalation_msg
        )


@tagged("post_install", "-at_install")
class TestUnsubscribePage(HamsHttpCase):

    def setUp(self):
        super().setUp()
        unique_id = str(uuid.uuid4())[:8]
        self.test_user = self.env["res.users"].create(
            {
                "name": f"Unsubscribe Test User {unique_id}",
                "login": f"unsub_{unique_id}",
                "password": "unsub_pw",
                "email": f"unsub_{unique_id}@example.com",
            }
        )

    def test_public_user_sees_login_prompt_not_lockout_form(self):
        self.authenticate(None, None)
        response = self.url_open("/unsubscribe")
        self.assertEqual(response.status_code, 200)
        msg = "[!] DIAGNOSTIC FOR AI: Public (anonymous) visitor to /unsubscribe should be told to log in, not shown the account-lockout form."
        self.assertIn(b"You must be logged in to lock out your account", response.content, msg)
        self.assertNotIn(b'action="/unsubscribe/lockout"', response.content, msg)

    def test_authenticated_user_sees_lockout_form(self):
        # Tests [@ANCHOR: hams_base:unsubscribe_page_template]
        self.authenticate(self.test_user.login, "unsub_pw")
        response = self.url_open("/unsubscribe")
        self.assertEqual(response.status_code, 200)
        msg = "[!] DIAGNOSTIC FOR AI: A logged-in, non-public user visiting /unsubscribe should see the account-lockout form."
        self.assertIn(b'action="/unsubscribe/lockout"', response.content, msg)

    def test_public_user_post_to_lockout_is_rejected_by_auth_user(self):
        # The route declares auth='user', so an unauthenticated POST never
        # reaches unsubscribe_lockout()'s own body at all -- Odoo's routing
        # layer redirects to /web/login before dispatch. That's what this
        # asserts; it is NOT exercising the controller's own
        # `return request.redirect('/')` fallback, which in practice is
        # unreachable code given auth='user' (a public-user session can
        # never authenticate past the route's own auth gate to hit it).
        self.authenticate(None, None)
        response = self.url_open(
            "/unsubscribe/lockout",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            allow_redirects=False,
        )
        msg = "[!] DIAGNOSTIC FOR AI: An unauthenticated POST to /unsubscribe/lockout (auth='user') should be redirected to login by Odoo's own routing layer, not reach the controller."
        self.assertIn(response.status_code, (301, 302, 303), msg)
        self.assertIn("/web/login", response.headers.get("Location", ""), msg)

    def test_authenticated_user_post_to_lockout_deactivates_and_logs_out(self):
        # Tests [@ANCHOR: hams_base:unsubscribe_lockout_success]
        self.authenticate(self.test_user.login, "unsub_pw")
        response = self.url_open(
            "/unsubscribe/lockout",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
        )
        self.assertEqual(response.status_code, 200)
        msg = "[!] DIAGNOSTIC FOR AI: Successful self-lockout should render the 'Account Locked' confirmation page."
        self.assertIn(b"Account Locked", response.content, msg)

        self.test_user.invalidate_recordset()
        deactivated_msg = "[!] DIAGNOSTIC FOR AI: POSTing to /unsubscribe/lockout while authenticated should deactivate the calling user's own account (user.active=False)."
        self.assertFalse(self.test_user.active, deactivated_msg)

        # The lockout also logs the session out -- a follow-up request on the
        # same session should now be treated as anonymous/public.
        followup = self.url_open("/unsubscribe")
        logout_msg = "[!] DIAGNOSTIC FOR AI: After self-lockout, the session should be logged out -- the next request should see the public/anonymous view, not the lockout form."
        self.assertIn(b"You must be logged in to lock out your account", followup.content, logout_msg)


@tagged("post_install", "-at_install")
class TestEmailPolicyPage(HamsHttpCase):
    # Tests [@ANCHOR: hams_base:email_policy_template]

    def test_email_policy_page_reachable(self):
        response = self.url_open("/email-policy")
        msg = "[!] DIAGNOSTIC FOR AI: /email-policy should be reachable (200 OK) to anonymous visitors -- it's a public disclosure page."
        self.assertEqual(response.status_code, 200, msg)
        self.assertIn(b"How We Use Email", response.content, msg)
