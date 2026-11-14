# -*- coding: utf-8 -*-
"""Log lines in hams_base must not carry a person's email address."""
from odoo.tests import tagged
from odoo.addons.hams_base.log_mask import mask_email
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

LOGGER_PREFIX = "odoo.addons.hams_base"


def _boom(*args, **kwargs):
    raise ValueError("send failed")


@tagged("post_install", "-at_install")
class TestLogMasksEmail(HamsTransactionCase):
    def test_mask_email_keeps_first_letter_and_domain(self):
        self.assertEqual(mask_email("jane@example.com"), "j***@example.com")
        self.assertEqual(mask_email("Jane Doe <jane@example.com>"), "j***@example.com")
        self.assertEqual(mask_email(None), "***")

    def test_unsubscribe_request_log_has_no_full_address(self):
        self.env["ir.config_parameter"].with_user(self.env.ref("base.user_admin").id).set_param("mail.bounce.alias", "auto-mail-failure")
        msg_dict = {"to": "auto-mail-failure@hams.com", "subject": "Unsubscribe me", "body": "stop", "email_from": "secretperson@example.com"}
        with self.assertLogs(LOGGER_PREFIX, level="INFO") as logs:
            self.env["mail.thread"].message_route("Unsubscribe me", msg_dict)
        text = "\n".join(logs.output)
        self.assertIn("unsubscribe", text.lower())
        self.assertNotIn("secretperson", text, "[!] DIAGNOSTIC FOR AI: the unsubscribe log line must mask the sender address.")

    def test_old_email_send_failure_log_has_no_full_address(self):
        user = self.env["res.users"].create({"name": "Mask Test", "login": "mask_old@example.com", "email": "mask_old_secret@example.com"})
        self.safe_patch_object(type(self.env["mail.mail"]), "send", _boom)
        with self.assertLogs(LOGGER_PREFIX, level="ERROR") as logs:
            user.write({"email": "mask_new@example.com"})
        self.assertNotIn("mask_old_secret", "\n".join(logs.output), "[!] DIAGNOSTIC FOR AI: the failure log must mask the old address.")

    def test_bounce_notice_failure_log_has_no_full_address(self):
        club = self.env["res.partner"].create({"name": "Mask Club", "is_company": True, "email": "club@example.com"})
        member = self.env["res.partner"].create({"name": "Mask Member", "email": "bouncer_secret@example.com", "parent_id": club.id})
        self.safe_patch_object(type(club), "message_post", _boom)
        with self.assertLogs(LOGGER_PREFIX, level="ERROR") as logs:
            member._message_receive_bounce("bouncer_secret@example.com", member)
        self.assertNotIn("bouncer_secret", "\n".join(logs.output), "[!] DIAGNOSTIC FOR AI: the failure log must mask the bounced address.")
