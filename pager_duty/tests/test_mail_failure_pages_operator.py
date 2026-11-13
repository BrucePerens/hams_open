# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A refused outgoing mail raises ONE pager incident, without any address in it. The mail server is mocked."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

VICTIM = "victim.person@example.org"
SOURCE = "mail_send_failure"


@tagged("standard", "post_install", "-at_install")
class TestMailFailurePagesOperator(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        # The reporter's own cursor joins this test's transaction and rolls back with it.
        mode = self.enter_registry_test_mode()
        mode.__enter__()
        self.addCleanup(mode.__exit__, None, None, None)
        self.mail = self.env["mail.mail"].create(
            {"subject": "Secret subject", "body_html": "<p>token=abc123</p>", "email_to": VICTIM,
             "email_from": "noreply@example.org"}
        )
        self.env.flush_all()

    def incidents(self):
        self.env.invalidate_all()
        return self.env["pager.incident"].search([("source", "=", SOURCE)])

    def fail(self, reason="554 Message rejected: Email address is not verified.", failure_type="mail_smtp"):
        self.mail._postprocess_sent_message([], [], failure_reason=reason, failure_type=failure_type)

    # Tests [@ANCHOR: pager_duty:mail_failure_pages_operator]
    # Tests [@ANCHOR: pager_duty:page_mail_failure]
    def test_a_refused_send_raises_one_alarm_with_no_address(self):
        self.fail(reason=f"554 rejected {VICTIM}")
        found = self.incidents()
        self.assertEqual(len(found), 1)
        self.assertEqual(found.severity, "high")
        for needle in (VICTIM, "example.org", "Secret subject", "abc123"):
            self.assertNotIn(needle, found.description)

    def test_repeated_failures_are_one_incident(self):
        for _ in range(3):
            self.fail()
        found = self.incidents()
        self.assertEqual(len(found), 1)
        self.assertGreaterEqual(found.occurrence_count, 1)

    def test_a_successful_send_raises_no_alarm(self):
        self.mail._postprocess_sent_message([], [])
        self.assertFalse(self.incidents())

    def test_a_bad_recipient_is_not_an_operator_alarm(self):
        self.fail(failure_type="mail_email_invalid")
        self.assertFalse(self.incidents())

    def test_a_broken_alarm_does_not_break_the_mail_queue(self):
        self.safe_patch("odoo.addons.pager_duty.models.mail_mail._service_env", side_effect=ValueError("boom"))
        self.fail()  # must not raise
        self.assertFalse(self.incidents())
