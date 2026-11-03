# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Real, testable coverage for hams_helpdesk/models/inbound_spam_filter.py --
built directly from the live spam/phishing tickets found 2026-10-01 (see
night_shift_todo/high/inbound-mail-ticket-ingestion-has-no-spam-phishing-
filter-e3a8f612.md) so this proves the actual reported patterns are
caught, not synthetic lookalikes, alongside clearly-legitimate fixtures
proving the heuristic does not flag ordinary support requests."""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from email import message_from_bytes

from odoo.addons.hams_helpdesk.models.inbound_spam_filter import (
    BULK_SCORE_THRESHOLD,
    STORED_TICKET_SCORE_THRESHOLD,
    bulk_mail_findings,
    detect_bulk_mail_signals,
    detect_inbound_spam_signals,
    html_shape,
    inbound_spam_reasons,
    mail_signals,
    own_authenticated_sender,
    ses_verdict_reasons,
    ses_verdicts,
)
from .bulk_mail_fixtures import (
    BULK_HEADERS,
    REAL_LEGITIMATE_NAMES,
    REAL_MISSED_SPAM_NAMES,
    SES_PASS_HEADERS,
    newsletter_body,
    raw_message,
)


@tagged("post_install", "-at_install", "standard")
class TestInboundSpamFilter(HamsTransactionCase):
    # ------------------------------------------------------------------
    # Real spam/phishing fixtures (tickets #4/#5/#7, #13, #15 from the
    # to-do's own table).
    # ------------------------------------------------------------------

    def test_01_domain_expiring_spam_is_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="You have domain(s) expiring soon",
            body_html="<p>Act now to renew your domain(s) before they expire.</p>",
        )
        self.assertTrue(reasons, "Near-duplicate domain-expiry spam must be flagged.")
        self.assertTrue(
            any("domain-renewal" in reason for reason in reasons),
            reasons,
        )

    def test_02_nda_secure_document_phishing_lure_is_flagged(self):
        # Ticket #15: "Non Disclosure Agreement. Secure document" --
        # ShareFile-impersonation credential-harvesting lure per the
        # answered question's own write-up.
        body = (
            '<p>Please review and sign this Non Disclosure Agreement via '
            'our ShareFile secure document portal.</p>'
            '<p><a href="https://sharefile-secure-login.example.net/sign">'
            'Open in ShareFile</a></p>'
        )
        reasons = detect_inbound_spam_signals(
            subject="Non Disclosure Agreement. Secure document for your review",
            body_html=body,
        )
        self.assertTrue(reasons, "The NDA/secure-document phishing lure must be flagged.")
        self.assertTrue(
            any("NDA" in reason for reason in reasons),
            reasons,
        )
        self.assertTrue(
            any("sharefile" in reason.lower() for reason in reasons),
            reasons,
        )

    def test_03_quickbooks_redirect_phishing_is_flagged(self):
        # Ticket #13: a QuickBooks-impersonation "negative review" scare
        # email whose actual link goes through a redirect tracker to a
        # non-Intuit domain -- the exact click.sleadtrack.com -> non-Intuit
        # redirect described in the to-do.
        body = (
            "<p>A customer has provided negative feedback about your "
            "QuickBooks account. "
            '<a href="https://click.sleadtrack.com/go?id=1">'
            "Click here to respond via QuickBooks Online</a></p>"
        )
        reasons = detect_inbound_spam_signals(
            subject="A customer has provided negative feedback on your QuickBooks listing",
            body_html=body,
        )
        self.assertTrue(reasons, "The QuickBooks redirect phishing email must be flagged.")
        self.assertTrue(
            any("quickbooks" in reason.lower() for reason in reasons),
            reasons,
        )

    def test_04_rfq_and_leadgen_commercial_spam_is_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="High-Potential RFQs in Computer Hardware & Electronics",
            body_html="<p>We found high-intent buyer leads matching your profile.</p>",
        )
        self.assertTrue(reasons)

    def test_05_app_conversion_marketing_spam_is_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="Turn your online store into an iOS & Android app",
            body_html="<p>Launch your own mobile app today.</p>",
        )
        self.assertTrue(reasons)

    def test_06_pending_violation_reports_scare_spam_is_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="Action Required: 5 Pending Violation Reports",
            body_html="<p>Your account has pending violations. Act now.</p>",
        )
        self.assertTrue(reasons)

    # ------------------------------------------------------------------
    # Legitimate-ticket fixtures: a genuine human inquiry must NEVER be
    # flagged -- this is the false-positive-risk coverage the to-do calls
    # for explicitly.
    # ------------------------------------------------------------------

    def test_07_genuine_membership_question_is_not_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="General question about membership",
            body_html="<p>Hi, I'm trying to renew my membership and the portal "
            "gave me an error. Can someone help?</p>",
        )
        self.assertEqual(reasons, [])

    def test_08_genuine_delivery_problem_report_is_not_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="Delivery problem report",
            body_html="<p>Mail to postmaster bounced for one of our list "
            "members, here is the bounce report.</p>",
        )
        self.assertEqual(reasons, [])

    def test_09_genuine_mention_of_a_brand_without_a_link_is_not_flagged(self):
        # A real customer mentioning a brand name in plain conversation
        # (no call-to-action link) must not be treated as impersonation --
        # see inbound_spam_filter.py's own "gated on an actual link"
        # rationale.
        reasons = detect_inbound_spam_signals(
            subject="Question about my FedEx package and PayPal receipt",
            body_html="<p>I paid via PayPal and my FedEx package never arrived, "
            "can you help me look into it?</p>",
        )
        self.assertEqual(reasons, [])

    def test_10_legitimate_brand_link_matching_its_own_domain_is_not_flagged(self):
        # A link that genuinely points at the brand's own real domain must
        # not be flagged as impersonation just because the brand is named,
        # regardless of who the message is actually from (a real colleague
        # sharing a real envelope, not DocuSign itself).
        reasons = detect_inbound_spam_signals(
            subject="Our DocuSign envelope for the sponsorship agreement",
            body_html='<p>Please sign via DocuSign: '
            '<a href="https://www.docusign.net/Signing/envelope123">Sign now</a></p>',
        )
        self.assertEqual(reasons, [])

    # ------------------------------------------------------------------
    # Hostile and edge-case input (the function's docstring promises it
    # never raises on malformed input, and it runs inside mail ingestion).
    # ------------------------------------------------------------------

    def test_11_malformed_href_does_not_raise_and_other_links_still_count(self):
        # urlsplit("http://[x/") raises ValueError("Invalid IPv6 URL"); one such href used to break the whole
        # function. The well-formed link in the same message must still be judged.
        body = (
            '<p>Your QuickBooks account: <a href="http://[x/">one</a> '
            '<a href="https://click.sleadtrack.com/go?id=1">two</a></p>'
        )
        reasons = detect_inbound_spam_signals(subject="Account notice", body_html=body)
        self.assertTrue(any("quickbooks" in reason.lower() for reason in reasons), reasons)

    def test_12_only_malformed_hrefs_flag_nothing(self):
        body = '<p>See <a href="http://[x/">this</a> and <a href="https://[::1/">that</a></p>'
        self.assertEqual(detect_inbound_spam_signals(subject="Hello", body_html=body), [])

    def test_13_none_subject_and_body_are_accepted(self):
        self.assertEqual(detect_inbound_spam_signals(subject=None, body_html=None), [])

    def test_14_unsuccessful_payment_lure_is_flagged(self):
        reasons = detect_inbound_spam_signals(
            subject="Unsuccessful payment for your subscription", body_html="<p>Update your card.</p>"
        )
        self.assertTrue(any("billing" in reason for reason in reasons), reasons)

    def test_15_lookalike_domains_are_not_the_brands_own_domain(self):
        # "evilintuit.com" ends with "intuit.com" as a string but is not a subdomain of it, and
        # "intuit.com.evil.example" merely starts with it. Both must be flagged as impersonation.
        for link in ("https://evilintuit.com/login", "https://intuit.com.evil.example/login"):
            reasons = detect_inbound_spam_signals(
                subject="Your QuickBooks invoice",
                body_html='<p>Pay in QuickBooks: <a href="%s">open</a></p>' % link,
            )
            self.assertTrue(any("quickbooks" in reason.lower() for reason in reasons), (link, reasons))

    def test_16_brand_subdomain_and_uppercase_host_match_the_real_domain(self):
        reasons = detect_inbound_spam_signals(
            subject="Your QuickBooks invoice",
            body_html='<p>Pay in QuickBooks: <a href="https://Accounts.INTUIT.com/pay">open</a></p>',
        )
        self.assertEqual(reasons, [])

    # ------------------------------------------------------------------
    # Amazon SES scan verdicts [@ANCHOR: test_ses_verdicts_flag_only_a_fail]
    # Tests [@ANCHOR: hams_helpdesk:ses_verdicts]
    # ------------------------------------------------------------------

    def _ses_message(self, spam, virus):
        raw = ("X-SES-Spam-Verdict: %s\r\nX-SES-Virus-Verdict: %s\r\nSubject: hi\r\n\r\nbody\r\n" % (spam, virus)).encode()
        return message_from_bytes(raw)

    def test_17_ses_verdicts_flag_only_a_fail(self):
        self.assertEqual(ses_verdict_reasons(ses_verdicts(self._ses_message("PASS", "PASS"))), [])
        self.assertEqual(ses_verdict_reasons(ses_verdicts(self._ses_message("GRAY", "PROCESSING_FAILED"))), [])
        spam = ses_verdict_reasons(ses_verdicts(self._ses_message("FAIL", "PASS")))
        self.assertEqual(len(spam), 1)
        self.assertIn("spam scan verdict: FAIL", spam[0])
        both = ses_verdict_reasons(ses_verdicts(self._ses_message("fail", "FAIL")))
        self.assertEqual(len(both), 2)

    def test_18_a_message_without_ses_headers_has_no_verdicts(self):
        message = message_from_bytes(b"Subject: hi\r\n\r\nbody\r\n")
        self.assertEqual(ses_verdicts(message), {})
        self.assertEqual(ses_verdict_reasons(ses_verdicts(message)), [])
        self.assertEqual(ses_verdict_reasons(None), [])

    def test_19_a_hostile_verdict_value_is_bounded(self):
        verdicts = ses_verdicts(self._ses_message("FAIL" + "x" * 500, "PASS"))
        self.assertLessEqual(len(verdicts["spam"]), 32)
        self.assertEqual(ses_verdict_reasons(verdicts), [])


@tagged("post_install", "-at_install", "standard")
class TestBulkMailSignals(HamsTransactionCase):
    """The weighted bulk/marketing rules and the single entry point inbound_spam_reasons(), against the real ticket
    names of hams_prod (ids 113-140) and synthetic legitimate mail. False positives are the named risk."""
    # Tests [@ANCHOR: hams_helpdesk:inbound_spam_reasons]

    def _signals(self, subject, headers=SES_PASS_HEADERS + BULK_HEADERS, **kw):
        return mail_signals(message_from_bytes(raw_message("info@hams.com", subject, headers=headers, **kw)))

    def test_20_the_real_missed_newsletters_are_flagged_with_their_headers(self):
        for subject in REAL_MISSED_SPAM_NAMES:
            reasons = inbound_spam_reasons(subject, newsletter_body(), self._signals(subject), {"spam": "PASS", "virus": "PASS"})
            self.assertTrue(reasons, subject)
            self.assertIn("bulk/marketing mail", reasons[0], subject)
            self.assertTrue(any("List-Unsubscribe" in r for r in reasons), reasons)

    def test_21_the_real_missed_newsletters_are_flagged_from_the_stored_ticket_alone(self):
        # The retroactive pass has the subject and the sanitised body only (no headers, no plain part).
        body = "<table><table><table><table><p>text</p></table></table></table></table><a>Unsubscribe</a>"
        for subject in REAL_MISSED_SPAM_NAMES:
            self.assertFalse(detect_bulk_mail_signals(subject, body), "the live threshold needs the headers: %s" % subject)
            self.assertTrue(detect_bulk_mail_signals(subject, body, threshold=STORED_TICKET_SCORE_THRESHOLD), subject)

    def test_22_real_legitimate_names_with_an_ordinary_body_are_never_flagged(self):
        plain = "<p>Feed has not updated for 31 minutes.</p>"
        for name in REAL_LEGITIMATE_NAMES:
            self.assertEqual(inbound_spam_reasons(name, plain, {}, {"spam": "PASS", "virus": "PASS"}), [], name)
            self.assertEqual(inbound_spam_reasons(name, plain, self._signals(name, headers=SES_PASS_HEADERS, html=False, body="Hello")), [], name)

    def test_23_a_clickbait_subject_alone_is_not_enough(self):
        for subject in REAL_MISSED_SPAM_NAMES:
            self.assertEqual(inbound_spam_reasons(subject, "<p>A short plain note from a person.</p>", {}), [], subject)
            self.assertEqual(
                detect_bulk_mail_signals(subject, "<p>Short.</p>", {"list_id": True}), [], "one header plus a subject stays below the threshold"
            )

    def test_24_a_vendor_notice_with_an_unsubscribe_link_is_not_flagged(self):
        body = "<table><tr><td><p>Your domain hams.com renews on 1 Dec.</p></td></tr></table><p><a>Unsubscribe</a></p>"
        signals = {"list_unsubscribe": True, "has_plain": True, "plain_chars": 400, "html_chars": 900, "tables": 1, "table_depth": 1}
        self.assertEqual(inbound_spam_reasons("Renewal notice", body, signals), [])

    def test_25_each_weak_signal_has_its_own_reason_and_weight(self):
        findings = bulk_mail_findings(
            REAL_MISSED_SPAM_NAMES[0],
            newsletter_body(),
            self._signals("x", headers=BULK_HEADERS + "Auto-Submitted: auto-generated\r\nList-Id: <l.example.net>\r\n"),
        )
        reasons = [reason for _weight, reason in findings]
        for expected in (
            "List-Unsubscribe", "Precedence: bulk", "List-Id", "Auto-Submitted: auto-generated", "no plain-text alternative",
            "tables nested", "images", "tracking image", "view in browser", "update your preferences", "clickbait",
        ):
            self.assertTrue(any(expected.lower() in reason.lower() for reason in reasons), (expected, reasons))
        self.assertTrue(all(0 < weight <= 2 for weight, _reason in findings))
        self.assertGreaterEqual(BULK_SCORE_THRESHOLD, 3, "no two weak signals may flag a message")

    def test_26_a_failed_dmarc_spf_dkim_adds_weight_only(self):
        failing = "Authentication-Results: amazonses.com; spf=fail smtp.mailfrom=x.example.net; dkim=fail header.i=@x.example.net; dmarc=fail header.from=x.example.net\r\n"
        signals = self._signals("Hello", headers=failing, html=False, body="Plain words from someone.")
        self.assertEqual((signals["spf"], signals["dkim"], signals["dmarc"]), ("fail", "fail", "fail"))
        self.assertEqual(inbound_spam_reasons("Hello", "<p>Plain words from someone.</p>", signals), [], "4 points of failures alone stay below 5")
        self.assertTrue(inbound_spam_reasons("Hello", "<p>Unsubscribe here</p>", signals))

    def test_27_only_the_ses_authentication_results_header_is_believed(self):
        forged = "Authentication-Results: mx.attacker.example; spf=pass; dkim=pass; dmarc=pass\r\n"
        signals = self._signals("Hello", headers=forged + SES_PASS_HEADERS.replace("pass", "fail"))
        self.assertEqual(signals["dmarc"], "fail", "the first header not written by amazonses.com is ignored")
        signals = self._signals("Hello", headers=forged)
        self.assertNotIn("dmarc", signals)

    def test_28_replies_members_and_our_own_authenticated_senders_are_exempt(self):
        subject = REAL_MISSED_SPAM_NAMES[0]
        signals = self._signals(subject)
        self.assertTrue(inbound_spam_reasons(subject, newsletter_body(), signals))
        self.assertFalse(detect_bulk_mail_signals(subject, newsletter_body(), dict(signals, reply_headers=True)), "a reply is not judged by the bulk rules")
        self.assertEqual(inbound_spam_reasons(subject, newsletter_body(), signals, trusted_sender=True), [])
        self.assertTrue(
            inbound_spam_reasons(subject, newsletter_body(), dict(signals, dmarc="fail"), trusted_sender=True),
            "a spoofed member address (DMARC fail) does not bypass the filter",
        )
        own = dict(signals, from_domain="hams.com", dkim="pass")
        self.assertTrue(own_authenticated_sender(own))
        self.assertEqual(inbound_spam_reasons(subject, newsletter_body(), own), [])
        forged = dict(signals, from_domain="hams.com", dkim="fail", dmarc="fail")
        self.assertFalse(own_authenticated_sender(forged))
        self.assertTrue(inbound_spam_reasons(subject, newsletter_body(), forged), "a forged From: hams.com is judged")
        self.assertFalse(own_authenticated_sender(dict(signals, from_domain="evil-hams.com", dkim="pass")))

    def test_29_the_new_lure_subjects_and_ses_fail_are_strong_signals(self):
        for subject in ("Your subscription renews soon", "Security alert for someone@gmail.com", "Trade receivable balance confirmation regarding the audit of K"):
            self.assertTrue(inbound_spam_reasons(subject, "<p>x</p>", {}), subject)
        self.assertTrue(inbound_spam_reasons("Question", "<p>x</p>", {}, {"spam": "FAIL"}))

    def test_30_hostile_input_never_raises(self):
        self.assertEqual(html_shape("<table>" * 3 + "</table>" * 9)["table_depth"], 3)
        self.assertEqual(html_shape("</table></table><table>")["table_depth"], 1)
        self.assertEqual(mail_signals(message_from_bytes(b"\xff\xfe not a message")).get("tables", 0), 0)
        self.assertEqual(inbound_spam_reasons(None, None, None, None), [])
        self.assertLessEqual(html_shape("<table>" * 200000)["tables"], 400000 // len("<table>") + 1)
