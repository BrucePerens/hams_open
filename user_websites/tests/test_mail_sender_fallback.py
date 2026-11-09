# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
"""
The mail templates of user_websites never use a made-up sender or recipient. They render each address field with the
company email unset and the mail server's default from address unset and then set, and check that no
`yourcompany.com` address appears.
"""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

_DEFAULT_FROM = "notifications@mail-server.example.org"


@tagged("post_install", "-at_install")
class TestMailSenderFallback(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.company = self.env.company
        self.company.email = False
        blog = self.env["blog.blog"].create({"name": "Sender fallback blog"})
        self.post = self.env["blog.post"].create({"name": "Sender fallback post", "blog_id": blog.id})
        self.report = self.env["content.violation.report"].create(
            {"target_url": "/sender-fallback", "description": "Test", "reported_by_email": "reporter@example.org"}
        )

    def _default_from(self, value):
        self.safe_patch_object(type(self.env["ir.mail_server"]), "_get_default_from_address", lambda _self: value)

    def _render(self, xmlid, field, record):
        template = self.env.ref(xmlid)
        return template._render_field(field, [record.id])[record.id]

    def _senders(self):
        return {
            "user_websites.email_template_pending_violations_summary": self._render(
                "user_websites.email_template_pending_violations_summary", "email_from", self.company
            ),
            "user_websites.email_template_weekly_digest": self._render(
                "user_websites.email_template_weekly_digest", "email_from", self.post
            ),
        }

    def test_no_company_email_and_no_server_default_renders_an_empty_sender_not_a_placeholder(self):
        # Tests [@ANCHOR: user_websites:mail_sender_fallback]
        self._default_from(None)
        for xmlid, sender in self._senders().items():
            with self.subTest(xmlid):
                self.assertFalse((sender or "").strip(), sender)
        recipient = self._render("user_websites.email_template_content_violation", "email_to", self.report)
        self.assertFalse((recipient or "").strip(), recipient)

    def test_the_mail_servers_default_from_address_is_the_fallback_sender(self):
        # Tests [@ANCHOR: user_websites:mail_sender_fallback]
        self._default_from(_DEFAULT_FROM)
        for xmlid, sender in self._senders().items():
            with self.subTest(xmlid):
                self.assertEqual(sender.strip(), _DEFAULT_FROM)

    def test_the_company_email_wins_over_the_server_default(self):
        # Tests [@ANCHOR: user_websites:mail_sender_fallback]
        self._default_from(_DEFAULT_FROM)
        self.company.email = "hq@club.example.org"
        self.env.user.company_id = self.company
        for xmlid, sender in self._senders().items():
            with self.subTest(xmlid):
                self.assertEqual(sender.strip(), "hq@club.example.org")
        recipient = self._render("user_websites.email_template_content_violation", "email_to", self.report)
        self.assertEqual(recipient.strip(), "hq@club.example.org")

    def test_no_template_renders_a_yourcompany_address(self):
        # Tests [@ANCHOR: user_websites:mail_sender_fallback]
        for default in (None, _DEFAULT_FROM):
            self._default_from(default)
            for xmlid, field, record in (
                ("user_websites.email_template_content_violation", "email_from", self.report),
                ("user_websites.email_template_content_violation", "email_to", self.report),
                ("user_websites.email_template_pending_violations_summary", "email_from", self.company),
                ("user_websites.email_template_weekly_digest", "email_from", self.post),
            ):
                with self.subTest(xmlid=xmlid, field=field, default=default):
                    self.assertNotIn("yourcompany.com", self._render(xmlid, field, record) or "")
