# -*- coding: utf-8 -*-
"""
Real coverage for res_users.py's write() override -- had zero test coverage before this file
(confirmed by grepping tests/ for a matching test). It sends a security-alert email to a user's OLD
address whenever their email or login changes; nothing had ever proven that email is actually
created, addressed to the right recipient, or that it survives a no-op write (same email, or an
unrelated field change) without being sent needlessly.
"""
from odoo.tests import tagged
from odoo.tools.translate import code_translations
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestResUsersEmailChangeAlert(HamsTransactionCase):
    def test_changing_email_creates_a_security_alert_mail_to_the_old_address(self):
        # Tests [@ANCHOR: hams_base:COMM_res_users_write]
        user = self.env["res.users"].create(
            {
                "name": "Email Change Test User",
                "login": "email_change_test@example.com",
                "email": "email_change_test@example.com",
            }
        )
        mails_before = self.env["mail.mail"].search_count([])
        user.write({"email": "new_address@example.com"})
        mails_after = self.env["mail.mail"].search([], order="id desc", limit=1)

        self.assertGreater(
            self.env["mail.mail"].search_count([]),
            mails_before,
            "Changing email must create a new mail.mail record.",
        )
        self.assertEqual(mails_after.email_to, "email_change_test@example.com")
        self.assertIn("new_address@example.com", mails_after.body_html)

    def test_an_unrelated_field_write_sends_no_alert(self):
        user = self.env["res.users"].create(
            {
                "name": "No Alert Test User",
                "login": "no_alert_test@example.com",
                "email": "no_alert_test@example.com",
            }
        )
        mails_before = self.env["mail.mail"].search_count([])
        user.write({"name": "Renamed User"})
        self.assertEqual(self.env["mail.mail"].search_count([]), mails_before)

    def test_writing_the_same_email_sends_no_alert(self):
        user = self.env["res.users"].create(
            {
                "name": "Same Email Test User",
                "login": "same_email_test@example.com",
                "email": "same_email_test@example.com",
            }
        )
        mails_before = self.env["mail.mail"].search_count([])
        user.write({"email": "same_email_test@example.com"})
        self.assertEqual(self.env["mail.mail"].search_count([]), mails_before)

    def test_the_alert_is_in_the_members_language(self):
        # Tests [@ANCHOR: hams_base:COMM_res_users_write]
        self.env["res.lang"]._activate_lang("fr_FR")
        user = self.env["res.users"].create(
            {
                "name": "French Alert User",
                "login": "fr_alert_test@example.com",
                "email": "fr_alert_test@example.com",
                "lang": "fr_FR",
            }
        )
        self.safe_patch_object(
            code_translations,
            "python_translations",
            {("hams_base", "fr_FR"): {"Security Alert: Your HAMS Email Address was Changed": "FR alerte"}},
        )
        user.write({"email": "fr_alert_new@example.com"})
        mail = self.env["mail.mail"].search([("email_to", "=", "fr_alert_test@example.com")], order="id desc", limit=1)
        self.assertEqual(mail.subject, "FR alerte")
