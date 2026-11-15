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
