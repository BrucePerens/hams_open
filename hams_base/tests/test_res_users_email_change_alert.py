# -*- coding: utf-8 -*-
"""
Real coverage for res_users.py's write() override -- had zero test coverage before this file
(confirmed by grepping tests/ for a matching test). It sends a security-alert email to a user's OLD
address whenever their email or login changes; nothing had ever proven that email is actually
created, addressed to the right recipient, or that it survives a no-op write (same email, or an
unrelated field change) without being sent needlessly.
"""
from unittest.mock import MagicMock

from odoo.exceptions import AccessError
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
        subjects = []

        def _capture_create(mail_model, vals_list):
            if isinstance(vals_list, dict):
                vals_list = [vals_list]
            subjects.extend(
                vals["subject"]
                for vals in vals_list
                if vals.get("email_to") == "fr_alert_test@example.com"
            )
            return mail_model.browse()

        self.safe_patch_object(type(self.env["mail.mail"]), "create", new=_capture_create)
        user.write({"email": "fr_alert_new@example.com"})
        self.assertIn("FR alerte", subjects, "the alert must be written in the member's language")

    def _user_whose_mail_service_is_broken(self, login):
        user = self.env["res.users"].create({"name": "Broken Mail User", "login": login, "email": login})
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_uid",
            side_effect=AccessError("simulated: mail service account unusable"),
        )
        return user

    def test_a_failed_security_alert_opens_a_pager_incident_and_the_change_is_saved(self):
        # Tests [@ANCHOR: hams_base:COMM_security_notice_failure_incident]
        user = self._user_whose_mail_service_is_broken("alert_fail@example.com")
        incidents = MagicMock()
        incidents.env.transaction = MagicMock()
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value={"pager.incident": incidents},
        )
        with self.assertLogs("odoo.addons.hams_base.models.res_users", level="ERROR"):
            user.write({"email": "alert_fail_new@example.com"})
        self.assertEqual(user.email, "alert_fail_new@example.com")
        incidents.report_incident.assert_called_once()
        vals = incidents.report_incident.call_args.args[0]
        self.assertIn(str(user.id), vals["description"])
        self.assertNotIn("alert_fail@example.com", vals["description"])

    def test_a_failed_pager_report_for_a_failed_alert_is_logged_not_raised(self):
        # Tests [@ANCHOR: hams_base:COMM_security_notice_failure_incident]
        user = self._user_whose_mail_service_is_broken("alert_fail2@example.com")
        with self.assertLogs("odoo.addons.hams_base.models.res_users", level="ERROR") as logs:
            user.write({"email": "alert_fail2_new@example.com"})
        self.assertEqual(user.email, "alert_fail2_new@example.com")
        self.assertTrue(any("security warning" in line for line in logs.output))
