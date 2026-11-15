# -*- coding: utf-8 -*-
"""A queued mail never goes to an address on mail.blacklist (SES bounce/complaint suppression)."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestMailBlacklist(HamsTransactionCase):
    # [@ANCHOR: test_a_blacklisted_recipient_is_dropped_before_send]
    # Tests [@ANCHOR: hams_base:mail_skips_blacklisted_recipients]
    def test_a_blacklisted_recipient_is_dropped_before_send(self):
        self.env["mail.blacklist"]._add("bounced@example.com")
        mail = self.env["mail.mail"].create(
            {"subject": "Digest", "body_html": "<p>x</p>", "email_to": "Bounced@Example.com, ok@example.com"}
        )
        mail._send()
        self.assertEqual(mail.state, "outgoing")
        self.assertEqual(mail.email_to, "ok@example.com")

        only = self.env["mail.mail"].create({"subject": "Digest", "body_html": "<p>x</p>", "email_to": "bounced@example.com"})
        only._send()
        self.assertEqual(only.state, "exception")

    # [@ANCHOR: test_send_filters_only_outgoing_mail]
    # Tests [@ANCHOR: hams_base:mail_send_filters_outgoing]
    def test_send_filters_only_outgoing_mail(self):
        self.env["mail.blacklist"]._add("bounced@example.com")
        done = self.env["mail.mail"].create(
            {"subject": "Old", "body_html": "<p>x</p>", "email_to": "bounced@example.com, ok@example.com"}
        )
        done.write({"state": "sent"})
        done._send()
        self.assertEqual(done.state, "sent")
        self.assertEqual(done.email_to, "bounced@example.com, ok@example.com")

    # [@ANCHOR: test_drop_keeps_other_recipients_and_partners]
    # Tests [@ANCHOR: hams_base:mail_drop_blacklisted_recipients]
    def test_drop_keeps_other_recipients_and_partners(self):
        self.env["mail.blacklist"]._add("bounced@example.com")
        bad = self.env["res.partner"].create({"name": "Bad", "email": "bounced@example.com"})
        good = self.env["res.partner"].create({"name": "Good", "email": "good@example.com"})
        mail = self.env["mail.mail"].create(
            {
                "subject": "Digest",
                "body_html": "<p>x</p>",
                "email_to": "bounced@example.com",
                "email_cc": "Bounced@Example.com, cc@example.com",
                "recipient_ids": [(6, 0, (bad | good).ids)],
            }
        )
        mail._drop_blacklisted_recipients()
        self.assertEqual(mail.state, "outgoing")
        self.assertFalse(mail.email_to)
        self.assertEqual(mail.email_cc, "cc@example.com")
        self.assertEqual(mail.recipient_ids, good)
