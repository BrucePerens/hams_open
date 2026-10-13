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

from odoo.addons.hams_helpdesk.models.inbound_spam_filter import (
    detect_inbound_spam_signals,
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
