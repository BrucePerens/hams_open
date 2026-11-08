# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Phishing links are defanged for staff (Bruce, 2026-11-08): in a ticket whose triage verdict is phishing the staff form,
the chatter and the notification email show links as plain text; tickets with any other verdict keep clickable links; the
stored text and the shared text filter are unchanged; a changed verdict or a person's override shows the right form at once."""
from markupsafe import Markup

from odoo.tests.common import tagged
from odoo.addons.mail.tools.discuss import Store
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from ..models.link_defang import DEFANG_WARNING_TEXT, defang_html, defang_url_text

PHISH_URL = "https://login-hams-com.evil.example/verify?token=1"
BODY = '<p>Your account is locked. <a href="%s">Verify now</a> or write to <a href="mailto:help@evil.example">help</a>.</p>' % PHISH_URL
LIVE = ('href="https://', 'href="mailto:', "href='https://")


@tagged("post_install", "-at_install", "standard")
class TestPhishingLinkDefangUnits(HamsTransactionCase):
    def test_01_a_url_becomes_inert_text(self):
        # Tests [@ANCHOR: hams_helpdesk:defang_url_text]
        self.assertEqual(defang_url_text("https://a.example.com/x?y=1"), "hxxps://a[.]example[.]com/x?y=1")
        self.assertEqual(defang_url_text("http://a.example"), "hxxp://a[.]example")
        self.assertEqual(defang_url_text("HTTP://A.EXAMPLE"), "hxxp://A[.]EXAMPLE")
        self.assertEqual(defang_url_text("ftp://f.example"), "fxp://f[.]example")
        self.assertEqual(defang_url_text("mailto:a@b.example"), "mailto:a[@]b[.]example")
        self.assertEqual(defang_url_text(None), "")
        self.assertEqual(defang_url_text("  "), "")

    def test_02_a_link_becomes_text_with_its_own_wording_and_the_defanged_address(self):
        # Tests [@ANCHOR: hams_helpdesk:defang_html]
        out = defang_html(BODY)
        self.assertIsInstance(out, Markup)
        self.assertNotIn("href", out)
        self.assertNotIn("<a ", out)
        self.assertIn("Verify now (hxxps://login-hams-com[.]evil[.]example/verify?token=1)", out)
        self.assertIn("help (mailto:help[@]evil[.]example)", out)
        self.assertIn("Your account is locked.", out)

    def test_03_a_link_whose_text_is_the_address_shows_the_address_once(self):
        # Tests [@ANCHOR: hams_helpdesk:defang_html]
        out = defang_html('<p><a href="https://e.example/a">https://e.example/a</a></p>')
        self.assertEqual(out.count("hxxps://e[.]example/a"), 1)
        self.assertNotIn("href", out)

    def test_04_urls_written_out_in_text_and_other_url_attributes_are_defanged_too(self):
        # Tests [@ANCHOR: hams_helpdesk:defang_html]
        out = str(
            defang_html(
                'See https://e.example/a and www.e.example now.<img src="https://t.example/p.gif">'
                '<form action="https://e.example/post"><button formaction="https://e.example/b">Go</button></form>'
                '<a href="javascript:alert(1)">x</a>'
            )
        )
        for live in ("https://", "http://", "www.", "src=", "action=", "formaction=", "href=", "<a "):
            self.assertNotIn(live, out)
        self.assertIn("hxxps://e[.]example/a", out)
        self.assertIn("www[.]e[.]example", out)

    def test_05_empty_and_unparseable_text_never_raises_and_never_links(self):
        # Tests [@ANCHOR: hams_helpdesk:defang_html]
        self.assertEqual(defang_html(""), Markup(""))
        self.assertEqual(defang_html(False), Markup(""))
        self.assertEqual(defang_html(None), Markup(""))
        out = defang_html("bad \x00 control http://e.example/x <a href='http://e.example'>y</a>")
        self.assertNotIn("http://", out)
        self.assertNotIn("<a", out)
        self.assertIn("hxxp://e[.]example/x", out)

    def test_06_markup_in_the_text_stays_text_not_markup(self):
        # Tests [@ANCHOR: hams_helpdesk:defang_html]
        out = defang_html("a &lt;script&gt;alert(1)&lt;/script&gt; b <b>bold</b>")
        self.assertNotIn("<script", out)
        self.assertIn("<b>bold</b>", out)

    def test_07_the_warning_text_is_in_the_staff_form_banner(self):
        # Tests [@ANCHOR: hams_helpdesk:view_phishing_banner]
        # Tests [@ANCHOR: hams_helpdesk:defang_warning_text]
        arch = " ".join(self.env["hams_helpdesk.ticket"].get_view(view_type="form")["arch"].split())
        self.assertIn("o_phishing_banner", arch)
        for phrase in ("Phishing warning", "cannot be clicked", "hxxp and hxxps stand for http and https", "Do not open them", "move it out of the Spam stage"):
            self.assertIn(phrase, arch)
            self.assertIn(phrase, DEFANG_WARNING_TEXT)
        for promise in ("privacy", "confidential", "anonym"):
            self.assertNotIn(promise, DEFANG_WARNING_TEXT.lower(), "ADR-0112: no promise of privacy in user-facing text")


@tagged("post_install", "-at_install", "standard")
class TestPhishingLinkDefangTickets(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.Ticket = self.env["hams_helpdesk.ticket"]
        self.admin = self.env.ref("base.user_admin")
        self.admin.email = "on-duty-admin@example.org"
        self.admin.notification_type = "email"
        self.agent_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_ai_triage_service")
        self.staff = self.env["res.users"].create(
            {
                "name": "Helpdesk reader",
                "login": "phishing-defang-reader",
                "email": "reader@example.org",
                "notification_type": "email",
                "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
            }
        )
        self.sender = self.env["res.partner"].create({"name": "Phisher", "email": "phisher@evil.example"})

    def _ticket(self, name="Account locked"):
        return self.Ticket.with_user(self.admin).create({"name": name, "description": BODY, "ticket_type": "general"})

    def _phishing_ticket(self, name="Account locked"):
        ticket = self._ticket(name)
        ticket.with_user(self.agent_uid).mcp_record_verdict("phishing", "Credential lure on a look-alike domain")
        return ticket

    def _store_body(self, message):
        """The body of ``message`` as the web client is sent it (the store data), a string."""
        store = Store()
        store.add(message)
        data = store.get_result()["mail.message"]
        record = data[message.id] if isinstance(data, dict) else next(r for r in data if r["id"] == message.id)
        value = record["body"]
        return str(value[1] if isinstance(value, list) else value)

    def _chatter_body(self, ticket, body=BODY):
        message = ticket.message_post(body=body, message_type="comment", subtype_xmlid="mail.mt_comment", author_id=self.sender.id)
        return self._store_body(message), message

    def test_01_a_phishing_ticket_shows_no_live_link_in_the_staff_description(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_links_defanged]
        # Tests [@ANCHOR: hams_helpdesk:compute_links_defanged]
        # Tests [@ANCHOR: hams_helpdesk:compute_description_defanged]
        # Tests [@ANCHOR: hams_helpdesk:ticket_links_defanged_fields]
        ticket = self._phishing_ticket()
        self.assertTrue(ticket.links_defanged)
        self.assertNotIn("href", ticket.description_defanged)
        self.assertIn("hxxps://login-hams-com[.]evil[.]example/verify?token=1", ticket.description_defanged)
        self.assertIn("href=", ticket.description, "the stored text keeps its links: the shared filter is unchanged")
        self.assertEqual(ticket.stage, "spam")

    def test_02_every_other_verdict_and_no_verdict_keep_clickable_links(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_links_defanged]
        # Tests [@ANCHOR: hams_helpdesk:compute_description_defanged]
        self.assertFalse(self._ticket().links_defanged, "no verdict yet")
        for verdict in ("real", "unsure", "spam"):
            ticket = self._ticket("Verdict %s" % verdict)
            ticket.with_user(self.agent_uid).mcp_record_verdict(verdict, "reason")
            self.assertFalse(ticket.links_defanged, verdict)
            self.assertIn('href="%s"' % PHISH_URL, ticket.description_defanged, verdict)
            body, _message = self._chatter_body(ticket)
            self.assertIn('href="%s"' % PHISH_URL, body, verdict)
        self.assertFalse(self.Ticket.browse().links_defanged)
        self.assertFalse((self._phishing_ticket("A") | self._phishing_ticket("B"))._triage_links_defanged(), "only one ticket at a time")

    def test_03_the_chatter_of_a_phishing_ticket_shows_no_live_link(self):
        # Tests [@ANCHOR: hams_helpdesk:message_to_store_defanged]
        # Tests [@ANCHOR: hams_helpdesk:message_defanged_ticket_ids]
        ticket = self._phishing_ticket()
        body, message = self._chatter_body(ticket)
        for live in LIVE:
            self.assertNotIn(live, body)
        self.assertIn("hxxps://login-hams-com[.]evil[.]example/verify?token=1", body)
        self.assertIn("href=", message.body, "the stored message keeps its link")

    def test_04_a_message_on_another_record_is_not_touched(self):
        # Tests [@ANCHOR: hams_helpdesk:message_to_store_defanged]
        partner = self.env["res.partner"].create({"name": "Someone"})
        message = partner.message_post(body=BODY, message_type="comment", subtype_xmlid="mail.mt_comment")
        body = self._store_body(message)
        self.assertIn('href="%s"' % PHISH_URL, body)
        self.assertNotIn("hxxps", body)

    def test_05_a_changed_verdict_shows_the_right_form_again(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_links_defanged]
        # Tests [@ANCHOR: hams_helpdesk:message_to_store_defanged]
        ticket = self._phishing_ticket()
        message_body, message = self._chatter_body(ticket)
        self.assertNotIn("href=", message_body)
        ticket._triage_record_verdict("real", "A person checked: a genuine member", "agent")
        self.assertFalse(ticket.links_defanged)
        self.assertIn("href=", ticket.description_defanged)
        self.assertIn("href=", self._store_body(message))
        ticket._triage_record_verdict("phishing", "Judged again", "agent")
        self.assertTrue(ticket.links_defanged)
        self.assertNotIn("href", ticket.description_defanged)
        self.assertNotIn("href=", self._store_body(message))

    def test_06_a_person_moving_the_ticket_out_of_spam_restores_the_links(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_links_defanged]
        # Tests [@ANCHOR: hams_helpdesk:triage_record_override]
        # Tests [@ANCHOR: hams_helpdesk:message_to_store_defanged]
        ticket = self._phishing_ticket()
        message_body, message = self._chatter_body(ticket)
        self.assertTrue(ticket.links_defanged)
        ticket.write({"stage": "new"})
        self.assertTrue(ticket.triage_overridden)
        self.assertEqual(ticket.triage_verdict, "phishing", "the verdict stays; the override is what restores the links")
        self.assertFalse(ticket.links_defanged)
        self.assertIn('href="%s"' % PHISH_URL, ticket.description_defanged)
        restored = self._store_body(message)
        self.assertIn('href="%s"' % PHISH_URL, restored)
        self.assertNotIn("hxxps://login", restored)

    def test_07_a_phishing_ticket_that_is_not_quarantined_still_defangs(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_links_defanged]
        utils_model = type(self.env["zero_sudo.security.utils"])
        real = utils_model._get_system_param

        def answer(utils, key, default=None):
            return "spam" if key == "hams_helpdesk.triage_quarantine_verdicts" else real(utils, key, default)

        self.safe_patch_object(utils_model, "_get_system_param", answer, create=True)
        ticket = self._phishing_ticket()
        self.assertEqual(ticket.stage, "new", "the setting leaves a phishing verdict in New")
        self.assertTrue(ticket.links_defanged)

    def _follower_mail(self, ticket, body=BODY):
        ticket.message_subscribe(partner_ids=[self.staff.partner_id.id])
        message = ticket.with_context(mail_notify_force_send=False).message_post(
            body=body, message_type="comment", subtype_xmlid="mail.mt_comment", author_id=self.sender.id,
            partner_ids=[self.staff.partner_id.id],
        )
        # The mail queue is read as the superuser: the test user is not allowed to read other people's outgoing mail.
        mails = self.env["mail.mail"].with_user(self.env.ref("base.user_root")).search([("mail_message_id", "=", message.id)])
        self.assertTrue(mails, "the control: a follower who is a staff user is mailed about the message")
        return "\n".join(mails.mapped("body_html"))

    def test_08_the_notification_email_of_a_phishing_ticket_has_no_live_link_and_the_warning(self):
        # Tests [@ANCHOR: hams_helpdesk:notify_layout_defanged]
        # Tests [@ANCHOR: hams_helpdesk:defang_warning_text]
        ticket = self._phishing_ticket()
        html = self._follower_mail(ticket)
        self.assertNotIn("login-hams-com.evil.example", html)
        self.assertNotIn("mailto:help@evil.example", html)
        self.assertIn("hxxps://login-hams-com[.]evil[.]example/verify?token=1", html)
        self.assertIn("Phishing warning", html)
        self.assertIn("Do not open them", html)
        self.assertIn("href=", html, "the layout's own link to open the ticket is kept")

    def test_09_the_email_of_a_normal_ticket_keeps_its_links_and_has_no_warning(self):
        # Tests [@ANCHOR: hams_helpdesk:notify_layout_defanged]
        ticket = self._ticket()
        ticket.with_user(self.agent_uid).mcp_record_verdict("real", "A member")
        html = self._follower_mail(ticket)
        self.assertIn('href="%s"' % PHISH_URL, html)
        self.assertNotIn("Phishing warning", html)

    def test_10_a_person_s_override_restores_the_links_in_the_email(self):
        # Tests [@ANCHOR: hams_helpdesk:notify_layout_defanged]
        ticket = self._phishing_ticket()
        ticket.write({"stage": "new"})
        html = self._follower_mail(ticket)
        self.assertIn('href="%s"' % PHISH_URL, html)
        self.assertNotIn("Phishing warning", html)

    def test_11_a_body_that_is_not_found_in_the_layout_defangs_the_whole_email(self):
        # Tests [@ANCHOR: hams_helpdesk:notify_layout_defanged]
        thread_model = type(self.env["mail.thread"])
        layout = Markup('<div><a href="https://evil.example/trap">trap</a> <a href="https://hams.com/web#id=1">Open</a></div>')
        self.safe_patch_object(thread_model, "_notify_by_email_render_layout", lambda *_a, **_k: layout, create=True)
        ticket = self._phishing_ticket()
        html = self._follower_mail(ticket)
        self.assertNotIn("href=", html, "fails safe: no link survives, the open-ticket button included")
        self.assertIn("hxxps://evil[.]example/trap", html)
