# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""
Coverage for content_moderation's own new contract, as extracted out of
user_websites on 2026-09-23. Deliberately does NOT re-test everything
user_websites' own (much larger) test suite already covers for this same
model -- report/strike mechanics reached through real suspension, group
moderation, GDPR export, multi-website routing, and the sanitizer's
automated-report path all live in real user_websites fixtures (slugged
users, user.websites.group records, published pages) and continue to run,
unchanged, in user_websites/tests/ against this same `_name` after the
extraction (moving a model to a new module, keeping its `_name`, doesn't
invalidate any of those tests -- the merged ORM class user_websites'
fixtures interact with still carries every field/method either module
contributes).

This file's job is narrower and genuinely new: prove the base module's OWN
contract holds when exercised on its own, without any consuming module's
extension fields or hook override present -- most importantly, that the
default _apply_enforcement_action() is a real, safe no-op rather than a
crash or a silently-skipped consequence.
"""
from types import SimpleNamespace

from odoo.exceptions import UserError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestContentViolationReport(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.owner = self.env["res.users"].create(
            {
                "name": "Reported Owner",
                "login": "cvr_owner_test",
                "email": "cvr_owner_test@example.com",
            }
        )
        self.reporter = self.env["res.users"].create(
            {
                "name": "Reporter",
                "login": "cvr_reporter_test",
                "email": "cvr_reporter_test@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    # Tests [@ANCHOR: content_moderation_content_violation_report_content_violation_report]
    def test_01_create_report(self):
        report = self.env["content.violation.report"].create(
            {
                "target_url": "/some/page",
                "description": "Inappropriate content",
                "content_owner_id": self.owner.id,
                "reported_by_user_id": self.reporter.id,
            }
        )
        self.assertEqual(report.state, "new")

    def test_02_target_url_and_description_required_non_empty(self):
        with self.assertRaises(Exception):
            self.env["content.violation.report"].create(
                {"target_url": "   ", "description": "Has a description"}
            )
        with self.assertRaises(Exception):
            self.env["content.violation.report"].create(
                {"target_url": "/some/page", "description": "   "}
            )

    def test_03_duplicate_report_same_url_same_reporter_blocked(self):
        self.env["content.violation.report"].create(
            {
                "target_url": "/dup/page",
                "description": "First report",
                "reported_by_user_id": self.reporter.id,
            }
        )
        with self.assertRaises(Exception):
            self.env["content.violation.report"].create(
                {
                    "target_url": "/dup/page",
                    "description": "Second report, same reporter/url",
                    "reported_by_user_id": self.reporter.id,
                }
            )

    def test_04_state_machine_mark_under_review_then_dismiss(self):
        report = self.env["content.violation.report"].create(
            {"target_url": "/x", "description": "d"}
        )
        report.action_mark_under_review()
        self.assertEqual(report.state, "under_review")
        report.action_dismiss()
        self.assertEqual(report.state, "dismissed")

    def test_05_state_guard_blocks_reprocessing_dismissed_report(self):
        report = self.env["content.violation.report"].create(
            {"target_url": "/x2", "description": "d"}
        )
        report.action_dismiss()
        with self.assertRaises(UserError):
            report.action_mark_under_review()
        with self.assertRaises(UserError):
            report.action_dismiss()

    def test_06_state_guard_blocks_reprocessing_actioned_report(self):
        # Tests [@ANCHOR: action_take_action_and_strike]
        # Deliberately no content_owner_id/content_group_id: this is a pure
        # state-machine test, and setting either would (when this suite runs
        # in the same shared test DB as user_websites, which installs an
        # _apply_enforcement_action() override -- see test_07's own comment
        # for why that's the realistic case, not a hypothetical) route
        # through that override's real strike/suspend side effects instead
        # of exercising only the state transition this test cares about.
        report = self.env["content.violation.report"].create(
            {"target_url": "/x3", "description": "d"}
        )
        report.action_take_action_and_strike()
        self.assertEqual(report.state, "action_taken")
        with self.assertRaises(UserError):
            report.action_mark_under_review()

    def test_07_take_action_is_a_no_op_by_default(self):
        # Tests [@ANCHOR: apply_enforcement_action]
        """
        The real point of this suite: content_moderation's own
        `_apply_enforcement_action()` default must be a safe no-op.

        Deliberately does NOT set content_owner_id: this test suite's own
        `test.py` run installs user_websites alongside content_moderation in
        the SAME database/registry, so user_websites' own
        `_apply_enforcement_action()` override (content_violation_report_
        moderation.py) is genuinely present and merged into this model's
        class -- there is no way to exercise "only the base module, no
        override installed" as a separate runtime here. What IS still
        genuinely testable, and is exactly what this base module's own
        design promises: a report whose content_owner_id/content_group_id
        neither resolve to a real target (e.g. a URL that didn't match any
        known slug) falls through user_websites' own override to ITS
        `else: super()._apply_enforcement_action()` branch, landing on this
        base module's real no-op -- proving the fallback the base module's
        own docstring promises actually fires, not just that it would in an
        install this codebase never actually exercises standalone.
        """
        report = self.env["content.violation.report"].create(
            {"target_url": "/x4", "description": "d"}
        )
        report.action_take_action_and_strike()  # must not raise
        self.assertEqual(report.state, "action_taken")
        self.assertTrue(
            any(
                "No enforcement handler is configured" in (m.body or "")
                for m in report.message_ids
            ),
            "The default hook should leave an audit-trail chatter note "
            "explaining that no consequence was applied.",
        )
        # Re-invoking is a no-op (state guard in action_take_action_and_strike
        # itself), not a second _apply_enforcement_action() call / second note.
        note_count_before = len(report.message_ids)
        report.action_take_action_and_strike()
        self.assertEqual(len(report.message_ids), note_count_before)


@tagged("post_install", "-at_install")
class TestGuestReportDedupe(HamsTransactionCase):
    # [@ANCHOR: content_moderation:test_guest_reports_are_deduplicated]
    # Tests [@ANCHOR: content_moderation:guest_dedupe_key]
    # Tests [@ANCHOR: content_moderation:create_guest_dedupe]
    def test_guest_reports_are_deduplicated(self):
        Report = self.env["content.violation.report"]
        guest = SimpleNamespace(env=SimpleNamespace(user=self.env.ref("base.public_user")))
        self.safe_patch("odoo.addons.content_moderation.models.content_violation_report.request", guest)
        ip = self.safe_patch_object(type(self.env["zero_sudo.security.utils"]), "_get_trusted_client_ip")

        def report(url, address):
            ip.return_value = address
            return Report.create({"target_url": url, "description": "spam"})

        first = report("/forum/post/1", "203.0.113.5")
        # Same network (/24), same URL, same day: the same report, not a second row.
        self.assertEqual(report("/forum/post/1", "203.0.113.77"), first)
        # Another URL, or another network, is a separate report.
        self.assertNotEqual(report("/forum/post/2", "203.0.113.5"), first)
        self.assertNotEqual(report("/forum/post/1", "198.51.100.9"), first)
        self.assertEqual(Report.search_count([("target_url", "=", "/forum/post/1")]), 2)
        # No address is stored: the key is a 64-character hash.
        self.assertEqual(len(first.guest_dedupe_key), 64)
        self.assertNotIn("203.0.113", first.guest_dedupe_key)

    # Tests [@ANCHOR: content_moderation:create_guest_dedupe]
    def test_requestless_reports_are_not_deduplicated(self):
        Report = self.env["content.violation.report"]
        # Without a web request (internal callers) nothing is deduplicated by this key.
        a = Report.create({"target_url": "/x", "description": "d"})
        b = Report.create({"target_url": "/x", "description": "d"})
        self.assertNotEqual(a, b)
        self.assertFalse(a.guest_dedupe_key)


@tagged("post_install", "-at_install")
class TestReportStateOnCreate(HamsTransactionCase):
    def test_portal_cannot_create_a_resolved_report(self):
        # Tests [@ANCHOR: content_moderation:create_guest_dedupe]
        portal = self.env["res.users"].create(
            {"name": "State Reporter", "login": "cvr_state_reporter", "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])]}
        )
        Report = self.env["content.violation.report"]
        report = Report.with_user(portal).create({"target_url": "/p", "description": "d", "state": "dismissed", "reported_by_user_id": portal.id})
        self.assertEqual(Report.browse(report.id).state, "new")
        resolved = Report.create({"target_url": "/q", "description": "d", "state": "dismissed"})
        self.assertEqual(resolved.state, "dismissed", "internal and service callers may still set it")
