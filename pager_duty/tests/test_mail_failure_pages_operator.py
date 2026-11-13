# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A refused outgoing mail raises ONE pager incident, without any address in it. The mail server is mocked."""
from odoo.addons.pager_duty.models import incident as incident_module
from odoo.addons.pager_duty.models.mail_mail import page_mail_failure
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
        # report_incident throttles a source for 60 s in Redis; an earlier test in the run may have used it up.
        if incident_module.redis and incident_module.redis_pool:
            incident_module.redis.Redis(connection_pool=incident_module.redis_pool).delete(f"pager_rate_limit:{SOURCE}:global")
            website = self.env["website"].get_current_website()
            incident_module.redis.Redis(connection_pool=incident_module.redis_pool).delete(f"pager_rate_limit:{SOURCE}:{website.id}")
        self.mail = self.env["mail.mail"].create(
            {"subject": "Secret subject", "body_html": "<p>token=abc123</p>", "email_to": VICTIM,
             "email_from": "noreply@example.org"}
        )
        self.env.flush_all()
        # Other tests in a combined run may have left an open incident of this source; the rule is "one open
        # incident", so these tests start from none.
        self.env.invalidate_all()
        self.env.cr.execute(  # audit-ignore-sql: constant text, test cleanup; Tested by [@ANCHOR: pager_duty:mail_failure_pages_operator]
            "UPDATE pager_incident SET status = 'resolved' WHERE source = %s", (SOURCE,)
        )
        self.env.invalidate_all()

    def pager(self):
        # The pager service account sees every incident, including ones an earlier test of the run left behind.
        return self.env["pager.incident"].with_user(self.env.ref("pager_duty.user_pager_service_internal"))

    def incidents(self):
        self.env.invalidate_all()
        return self.pager().search([("source", "=", SOURCE), ("status", "=", "open")])

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
        class BrokenRegistry:
            def cursor(self):
                raise ValueError("boom")

        page_mail_failure(BrokenRegistry(), "mail_smtp")  # must not raise
        self.assertFalse(self.incidents())
