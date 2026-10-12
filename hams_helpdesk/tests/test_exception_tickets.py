# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Automatic tickets for unhandled server exceptions (models/exception_fingerprint.py).

The reporter is exercised through its real entry points: ``report_exception`` for the
classification, scrubbing, de-duplication and storm tests, ``/web/dataset/call_kw`` over HTTP for the
ir.http hook, and ``ir.cron._callback`` for the cron hook.
"""
import importlib.util
import json
import logging
import os
import tempfile
from datetime import timedelta

import psycopg2.errors
import werkzeug.exceptions
from odoo import fields
from odoo.addons.base.models.ir_qweb import QWebError, QWebErrorInfo
from odoo.exceptions import AccessDenied, AccessError, MissingError, UserError, ValidationError
from odoo.tests.common import tagged
from odoo.tools import mute_logger
from odoo.tools.safe_eval import safe_eval

from odoo.addons.hams_helpdesk.controllers import portal as helpdesk_portal
from odoo.addons.hams_helpdesk.models import exception_fingerprint as reporter
from odoo.addons.hams_helpdesk.models import exception_scrub as scrub
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)

LETTERS = "abcdefgh"

SECRET_MESSAGE = (
    "bad value for k6bp@example.com from 203.0.113.9 and 2001:db8::1 password=hunter2 "
    "token=abcdef0123456789abcdef0123456789 callsign K6BP phone 15551234567 "
    "see https://hams.com/x?session_id=zzz&email=a@b.com"
)


def raise_attribute_error(message="boom"):
    """A real raise site, so the innermost frame is this function (an addon frame)."""
    raise AttributeError(message)


def raise_other_attribute_error():
    raise AttributeError("boom")


def raise_type_error(message):
    raise TypeError(message)


def raise_qweb_error():
    try:
        raise_type_error("in a template")
    except TypeError as inner:
        info = QWebErrorInfo("Error while rendering", None, None, None, None, [], "")
        raise QWebError(info) from inner


def raise_chained(message):
    try:
        raise_type_error(message)
    except TypeError as inner:
        raise ValueError("outer") from inner


class FakeClock:
    """Stands in for the ``time`` module inside the reporter only, so the process-wide clock is
    untouched."""

    def __init__(self, now):
        self.now = now

    def monotonic(self):
        return self.now


class ExceptionTicketCase(HamsTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.Fingerprint = cls.env["hams_helpdesk.exception.fingerprint"]
        cls.Ticket = cls.env["hams_helpdesk.ticket"]

    def setUp(self):
        super().setUp()
        # New cursors (the reporter's own) join this test's transaction and roll back with it.
        registry_test_mode = self.enter_registry_test_mode()
        registry_test_mode.__enter__()
        self.addCleanup(registry_test_mode.__exit__, None, None, None)
        # The gate is per-process state; start every test with it empty.
        reporter._GATE.clear()
        reporter._RECENT_WRITES.clear()
        self.addCleanup(reporter._GATE.clear)
        self.addCleanup(reporter._RECENT_WRITES.clear)

    def caught(self, raiser, *args):
        """The exception ``raiser`` raises, with its traceback (assertRaises strips it)."""
        try:
            raiser(*args)
        except Exception as exc:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:exc_describe]
            _logger.info("provoked %r", exc)
            return exc
        self.fail("nothing was raised")

    def clock(self, now):
        return self.safe_patch_object(reporter, "time", new=FakeClock(now))

    def report(self, exc, source="rpc", **kwargs):
        kwargs.setdefault("logged_in", True)
        kwargs.setdefault("uid", self.env.uid)
        kwargs.setdefault("path", "/web/dataset/call_kw/res.partner.write")
        # The reporter writes through its own environment: push this test's pending writes to the
        # database first, and drop this test's cache afterwards so it reads what the reporter wrote.
        self.env.flush_all()
        written = reporter.report_exception(self.env.registry, exc, source, **kwargs)
        self.env.invalidate_all()
        return written

    def fingerprints(self):
        return self.Fingerprint.search([])


@tagged("post_install", "-at_install", "standard")
class TestExceptionClassification(ExceptionTicketCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_is_expected]
    # Tests [@ANCHOR: hams_helpdesk:exc_report_exception]

    def test_expected_control_flow_is_not_reported(self):
        expected = [
            (werkzeug.exceptions.NotFound(), "http", False),
            (werkzeug.exceptions.Forbidden(), "http", True),
            (werkzeug.exceptions.BadRequest("CSRF token missing"), "http", True),
            (werkzeug.exceptions.HTTPException(response=None), "http", False),
            (UserError("shown to the user"), "rpc", True),
            (ValidationError("shown to the user"), "rpc", True),
            (AccessDenied(), "http", False),
            (AccessError("anonymous probe"), "http", False),
            (AccessError("permission dialog"), "rpc", True),
            (MissingError("deleted record"), "http", True),
            (BrokenPipeError(), "http", True),
        ]
        for exc, source, logged_in in expected:
            with self.subTest(exc=repr(exc), source=source):
                self.assertFalse(self.report(exc, source, logged_in=logged_in))
        self.assertFalse(self.fingerprints())

    def test_bot_probes_are_not_reported_for_anonymous_callers(self):
        exc = self.caught(raise_attribute_error)
        self.assertFalse(self.report(exc, "http", logged_in=False, path="/wp-login.php"))
        self.assertFalse(
            self.report(exc, "http", logged_in=False, path="/x", user_agent="Googlebot/2.1")
        )
        self.assertFalse(self.fingerprints())

    def test_real_failures_are_reported(self):
        reported = [
            (self.caught(raise_attribute_error), "rpc", True),
            (KeyError("k"), "rpc", True),
            (TypeError("bad"), "http", True),
            (psycopg2.errors.UniqueViolation("duplicate key"), "rpc", True),
            # The /community case: a logged-in user's page controller raising AccessError.
            (AccessError("public_directory_view"), "http", True),
            (UserError("a cron job has nobody to show it to"), "cron", True),
        ]
        for index, (exc, source, logged_in) in enumerate(reported):
            with self.subTest(exc=repr(exc)):
                reporter._GATE.clear()
                self.assertTrue(
                    self.report(exc, source, logged_in=logged_in, path=f"/r{LETTERS[index]}/p"),
                    repr(exc),
                )

    def test_anonymous_access_error_on_a_page_is_not_reported(self):
        self.assertFalse(self.report(AccessError("no"), "http", logged_in=False, path="/community"))


@tagged("post_install", "-at_install", "standard")
class TestExceptionScrubbing(ExceptionTicketCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_scrub_text]
    # Tests [@ANCHOR: hams_helpdesk:exc_scrubbed_trace]
    # Tests [@ANCHOR: hams_helpdesk:exc_frame_lines]
    # Tests [@ANCHOR: hams_helpdesk:exc_normalize_route]
    # Tests [@ANCHOR: hams_helpdesk:exc_ticket_body]

    FORBIDDEN = (
        "k6bp@example.com", "203.0.113.9", "2001:db8", "hunter2", "abcdef0123456789",
        "K6BP", "15551234567", "session_id=zzz", "a@b.com", "x?session",
    )

    def assert_clean(self, text):
        for needle in self.FORBIDDEN:
            self.assertNotIn(needle, text)

    # Tests [@ANCHOR: hams_helpdesk:exc_root_cause]
    # Tests [@ANCHOR: hams_helpdesk:exc_raised_in_safe_eval]
    def test_safe_eval_wrapper_is_looked_through(self):
        wrapped = self.caught(safe_eval, "{}['missing']")
        self.assertIsInstance(wrapped, ValueError)
        self.assertIs(type(scrub.root_cause(wrapped)), KeyError)
        type_name, _module, _frame = scrub.describe(wrapped)
        self.assertEqual(type_name, "builtins.KeyError")
        qweb = self.caught(raise_qweb_error)
        self.assertIs(type(scrub.root_cause(qweb)), TypeError)
        plain = self.caught(raise_type_error, "x")
        self.assertIs(scrub.root_cause(plain), plain)

    def test_message_is_scrubbed(self):
        trace = scrub.scrubbed_trace(self.caught(raise_attribute_error, SECRET_MESSAGE))
        self.assert_clean(trace)
        self.assertIn("AttributeError", trace)
        self.assertIn("raise_attribute_error", trace)

    def test_message_of_a_data_carrying_class_is_withheld(self):
        trace = scrub.scrubbed_trace(KeyError("k6bp@example.com"))
        self.assert_clean(trace)
        self.assertIn("<message withheld>", trace)
        pg_error = psycopg2.errors.UniqueViolation("Key (email)=(k6bp@example.com) already exists.")
        self.assert_clean(scrub.scrubbed_trace(pg_error))

    def test_chained_cause_is_included_and_scrubbed(self):
        trace = scrub.scrubbed_trace(self.caught(raise_chained, SECRET_MESSAGE))
        self.assert_clean(trace)
        self.assertIn("ValueError", trace)
        self.assertIn("TypeError: bad value for <email>", trace)
        self.assertIn("raise_type_error", trace)

    def test_route_drops_query_string_and_variable_parts(self):
        self.assertEqual(
            scrub.normalize_route("/community/k6bp?email=a@b.com&token=zzz", "/community/<string:slug>"),
            "/community/<string:slug>",
        )
        self.assertEqual(scrub.normalize_route("/some/page-12345/slug?x=1"), "/some/<x>/slug")
        self.assertEqual(
            scrub.normalize_route("/web/dataset/call_kw/res.partner.write?x=1", "/web/dataset/call_kw/<path:path>"),
            "/web/dataset/call_kw/res.partner.write",
        )

    def test_ticket_and_row_hold_nothing_forbidden(self):
        exc = self.caught(raise_attribute_error, SECRET_MESSAGE)
        self.assertTrue(
            self.report(exc, "http", path="/community/k6bp?email=a@b.com", rule="/community/<string:slug>")
        )
        record = self.fingerprints()
        ticket = record.ticket_id
        stored = " ".join(
            str(value)
            for value in (
                record.samples_json, record.route, record.frame, ticket.name, ticket.description,
                ticket.partner_id.id, ticket.callsign,
            )
        )
        self.assert_clean(stored)
        self.assertFalse(ticket.partner_id)
        self.assertEqual(json.loads(record.samples_json)[0]["uid"], self.env.uid)
        self.assertEqual(record.route, "/community/<string:slug>")
        self.assertIn("raise_attribute_error", ticket.description)


@tagged("post_install", "-at_install", "standard")
class TestExceptionDedupAndStorm(ExceptionTicketCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_admit]
    # Tests [@ANCHOR: hams_helpdesk:exc_settle]
    # Tests [@ANCHOR: hams_helpdesk:exc_fingerprint]
    # Tests [@ANCHOR: hams_helpdesk:exc_describe]
    # Tests [@ANCHOR: hams_helpdesk:exc_record_occurrence]
    # Tests [@ANCHOR: hams_helpdesk:exc_sync_ticket]
    # Tests [@ANCHOR: hams_helpdesk:exc_write_report]
    # Tests [@ANCHOR: hams_helpdesk:exc_service_env]

    def test_same_failure_is_one_row_and_one_ticket(self):
        exc = self.caught(raise_attribute_error)
        self.assertTrue(self.report(exc))
        record = self.fingerprints()
        self.assertEqual(len(record), 1)
        self.assertEqual(record.occurrence_count, 1)
        self.assertEqual(record.ticket_id.ticket_type, "server_exception")
        self.assertEqual(record.module, "hams_helpdesk")
        self.assertIn("test_exception_tickets:", record.frame)
        self.assertFalse(self.report(exc), "second hit within the minute must not write")
        self.assertEqual(self.fingerprints().occurrence_count, 1)

    def test_different_frame_or_route_is_a_different_fingerprint(self):
        self.assertTrue(self.report(self.caught(raise_attribute_error)))
        self.assertTrue(self.report(self.caught(raise_other_attribute_error)))
        self.assertTrue(self.report(self.caught(raise_attribute_error), path="/web/dataset/call_kw/res.partner.read"))
        self.assertEqual(len(self.fingerprints()), 3)

    def test_storm_writes_once_then_folds_the_count_into_the_next_write(self):
        exc = self.caught(raise_attribute_error)
        self.clock(1000.0)
        writes = sum(1 for _ in range(10000) if self.report(exc))
        self.assertEqual(writes, 1)
        record = self.fingerprints()
        self.assertEqual(record.occurrence_count, 1)
        self.assertEqual(len(self.Ticket.search([("ticket_type", "=", "server_exception")])), 1)
        # A minute later, in this process and in the database: one more write carries all 9,999
        # refused hits plus this one.
        record.last_written_at = fields.Datetime.now() - reporter.timedelta(seconds=120)
        reporter.time.now = 1061.0
        self.assertTrue(self.report(exc))
        self.assertEqual(self.fingerprints().occurrence_count, 10001)

    def test_release_marker_is_a_string(self):
        self.assertIsInstance(self.Fingerprint._release_marker(), str)

    def test_another_worker_that_wrote_a_moment_ago_wins(self):
        exc = self.caught(raise_attribute_error)
        self.clock(1000.0)
        self.assertTrue(self.report(exc))
        # This process's gate is open again, but the row says another worker just wrote it.
        reporter.time.now = 2000.0
        self.assertFalse(self.report(exc))
        self.assertEqual(self.fingerprints().occurrence_count, 1)

    def test_process_wide_cap(self):
        self.clock(1000.0)
        self.safe_patch_object(reporter, "GLOBAL_WRITES_PER_MINUTE", new=3)
        results = [self.report(KeyError("k"), path=f"/r{LETTERS[index]}/p") for index in range(6)]
        self.assertEqual(results.count(True), 3)

    def test_new_ticket_cap_keeps_counting_without_a_ticket(self):
        exc = self.caught(raise_attribute_error)
        self.safe_patch_object(reporter, "NEW_TICKETS_PER_HOUR", new=0)
        self.assertTrue(self.report(exc))
        record = self.fingerprints()
        self.assertEqual(record.occurrence_count, 1)
        self.assertFalse(record.ticket_id)


@tagged("post_install", "-at_install", "standard")
class TestExceptionNeverRaises(ExceptionTicketCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_report_exception]

    def test_writer_failure_is_swallowed_and_logged(self):
        exc = self.caught(raise_attribute_error)
        self.safe_patch_object(reporter, "_write_report", side_effect=RuntimeError("db is gone"))
        with self.assertLogs("hams_helpdesk.exception_reporter", level="ERROR") as logs:
            self.assertFalse(self.report(exc))
        self.assertIn("db is gone", logs.output[0])

    def test_a_failure_in_the_writer_is_not_reported_again(self):
        exc = self.caught(raise_attribute_error)
        calls = []

        def failing_writer(registry, data, folded):
            calls.append(1)
            # The writer itself hits an unhandled exception and "reports" it: must be refused.
            self.assertFalse(self.report(KeyError("nested"), path="/nested"))
            raise RuntimeError("writer broke")

        self.safe_patch_object(reporter, "_write_report", side_effect=failing_writer)
        with self.assertLogs("hams_helpdesk.exception_reporter", level="ERROR"):
            self.assertFalse(self.report(exc))
        self.assertEqual(len(calls), 1)
        self.assertFalse(self.fingerprints())

    def test_a_missing_service_account_does_not_raise(self):
        exc = self.caught(raise_attribute_error)
        self.safe_patch_object(reporter, "SERVICE_XMLID", new="hams_helpdesk.no_such_service_account")
        with self.assertLogs("hams_helpdesk.exception_reporter", level="ERROR"):
            self.assertFalse(self.report(exc))


@tagged("post_install", "-at_install", "standard")
class TestExceptionReopen(ExceptionTicketCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_release_marker]

    def setUp(self):
        super().setUp()
        self.marker = {"value": "A"}
        self.safe_patch_object(
            type(self.Fingerprint), "_release_marker", side_effect=lambda *args: self.marker["value"]
        )

    def occur_again(self, exc, marker):
        self.fingerprints().last_written_at = fields.Datetime.now() - reporter.timedelta(seconds=120)
        reporter._GATE.clear()
        self.marker["value"] = marker
        return self.report(exc)

    def test_closed_ticket_reopens_only_after_a_release(self):
        exc = self.caught(raise_attribute_error)
        self.assertTrue(self.report(exc))
        ticket = self.fingerprints().ticket_id
        ticket.write({"stage": "closed"})
        # Same release: counted, but left closed.
        self.assertTrue(self.occur_again(exc, "A"))
        self.assertEqual(ticket.stage, "closed")
        self.assertEqual(self.fingerprints().occurrence_count, 2)
        # After a release: reopened, with a note, and still the one ticket.
        self.assertTrue(self.occur_again(exc, "B"))
        self.assertEqual(ticket.stage, "new")
        self.assertEqual(len(self.Ticket.search([("ticket_type", "=", "server_exception")])), 1)
        self.assertIn("reopened", ticket.message_ids[0].body)


@tagged("post_install", "-at_install", "standard")
class TestExceptionTicketAccess(ExceptionTicketCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_fingerprint_list]

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.portal_user = cls.env["res.users"].create(
            {
                "name": "Exception Portal",
                "login": "exception_portal",
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )
        cls.staff = cls.env["res.users"].create(
            {
                "name": "Exception Staff",
                "login": "exception_staff",
                "group_ids": [(6, 0, [cls.env.ref("hams_helpdesk.group_helpdesk_user").id])],
            }
        )

    def test_staff_see_it_and_portal_cannot(self):
        self.assertTrue(self.report(self.caught(raise_attribute_error)))
        ticket = self.fingerprints().ticket_id
        self.assertEqual(ticket.ticket_type, "server_exception")
        as_staff = ticket.with_user(self.staff)
        self.assertEqual(as_staff.read(["ticket_type"])[0]["ticket_type"], "server_exception")
        rows = self.Fingerprint.with_user(self.staff).search([])
        self.assertEqual(rows.occurrence_count, 1)
        portal_tickets = self.Ticket.with_user(self.portal_user).search([("id", "=", ticket.id)])
        self.assertFalse(portal_tickets)
        with self.assertRaises(AccessError):
            self.Fingerprint.with_user(self.portal_user).search([])

    def test_staff_list_shows_rows_whose_ticket_is_assigned_to_someone_else(self):
        self.assertTrue(self.report(self.caught(raise_attribute_error)))
        record = self.fingerprints()
        other = self.env["res.users"].create({"name": "Other Staff", "login": "exception_other_staff"})
        record.ticket_id.write({"user_id": other.id})
        self.env.flush_all()
        self.env.invalidate_all()
        specification = {
            "name": {},
            "occurrence_count": {},
            "ticket_stage": {},
            "ticket_id": {"fields": {"display_name": {}}},
        }
        result = self.Fingerprint.with_user(self.staff).web_search_read([], specification)
        self.assertEqual(result["length"], 1)
        self.assertEqual(result["records"][0]["occurrence_count"], 1)

    def test_portal_cannot_choose_the_category(self):
        self.assertIn("server_exception", helpdesk_portal._PORTAL_EXCLUDED_TICKET_TYPES)

    def test_external_ai_triage_account_cannot_see_it(self):
        self.assertTrue(self.report(self.caught(raise_attribute_error)))
        ticket = self.fingerprints().ticket_id
        triage = self.env["zero_sudo.security.utils"]._get_service_env("hams_helpdesk.user_ai_triage_service")
        self.assertFalse(triage["hams_helpdesk.ticket"].search([("id", "=", ticket.id)]))

    def test_service_account_cannot_touch_other_tickets(self):
        service = self.env["zero_sudo.security.utils"]._get_service_env(reporter.SERVICE_XMLID)
        general = self.Ticket.create({"name": "A member's ticket", "ticket_type": "general"})
        self.assertFalse(service["hams_helpdesk.ticket"].search([("id", "=", general.id)]))
        with self.assertRaises(AccessError):
            service["hams_helpdesk.ticket"].create({"name": "Forged", "ticket_type": "general"})

    def test_no_triage_wakeup_for_exception_tickets(self):
        wakeup = self.safe_patch("odoo.addons.hams_helpdesk.models.helpdesk_ticket.write_triage_wakeups")
        self.assertTrue(self.report(self.caught(raise_attribute_error)))
        self.env.cr.postcommit.run()
        wakeup.assert_not_called()


@tagged("post_install", "-at_install", "standard")
class TestMailFailureReports(ExceptionTicketCase):
    """A message that fails to send is recorded by Odoo on the mail and not raised; it is reported
    from ``_postprocess_sent_message`` with counts and fixed labels only."""

    # Tests [@ANCHOR: hams_helpdesk:exc_report_event]
    # Tests [@ANCHOR: hams_helpdesk:exc_mail_postprocess]
    # Tests [@ANCHOR: hams_helpdesk:exc_report_mail_failure]
    # Tests [@ANCHOR: hams_helpdesk:exc_mail_failure_class]

    VICTIM = "victim.person@example.org"
    SUBJECT = "Confidential subject line 8675309"
    BODY = "Dear Alice Example, your secret is hunter2"

    def make_mail(self, **extra):
        values = {
            "subject": self.SUBJECT,
            "body_html": f"<p>{self.BODY}</p>",
            "email_to": self.VICTIM,
            "email_from": "noreply@example.org",
        }
        values.update(extra)
        return self.env["mail.mail"].create(values)

    def mail_rows(self):
        return self.Fingerprint.search([("source", "=", "mail")])

    def test_failed_send_makes_one_ticket_with_no_addresses_or_content(self):
        mail = self.make_mail()
        self.env.flush_all()
        reason = f"Connection refused talking to smtp.example.org for {self.VICTIM}: {self.BODY}"
        mail._postprocess_sent_message([], [], failure_reason=reason, failure_type="mail_smtp")
        self.env.invalidate_all()
        rows = self.mail_rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.exc_type, "mail.mail.send_failure")
        self.assertIn("mail_smtp/connect", rows.frame)
        self.assertEqual(rows.ticket_id.ticket_type, "server_exception")
        stored = " ".join(
            str(value)
            for value in (
                rows.name, rows.frame, rows.route, rows.samples_json, rows.ticket_id.name,
                rows.ticket_id.description,
            )
        )
        for needle in (self.VICTIM, "example.org", "Alice", "hunter2", "Confidential", "8675309", "smtp.example"):
            self.assertNotIn(needle, stored)
        self.assertIn("messages in this batch: 1", stored)

    def test_same_failure_class_is_de_duplicated_and_other_class_is_not(self):
        mail = self.make_mail()
        self.env.flush_all()
        mail._postprocess_sent_message([], [], failure_reason="Connection refused", failure_type="mail_smtp")
        mail._postprocess_sent_message([], [], failure_reason="connection reset", failure_type="mail_smtp")
        mail._postprocess_sent_message([], [], failure_reason="535 Authentication failed", failure_type="mail_smtp")
        self.env.invalidate_all()
        rows = self.mail_rows()
        self.assertEqual(len(rows), 2)
        self.assertEqual(len(rows.mapped("ticket_id")), 2)
        # The second connect failure was refused by the gate and is counted for the next write.
        pending = [entry[1] for entry in reporter._GATE.values()]
        self.assertEqual(sorted(pending), [0, 1])

    def test_bad_recipient_failures_are_not_tickets(self):
        mail = self.make_mail(email_to="not an address")
        self.env.flush_all()
        for failure_type in sorted(scrub.MAIL_FAILURES_NOT_REPORTED):
            mail._postprocess_sent_message([], [], failure_reason="x", failure_type=failure_type)
        self.env.invalidate_all()
        self.assertFalse(self.mail_rows())

    def test_successful_postprocess_reports_nothing(self):
        mail = self.make_mail()
        self.env.flush_all()
        mail._postprocess_sent_message([], [])
        self.env.invalidate_all()
        self.assertFalse(self.mail_rows())

    def test_real_send_failure_is_reported_through_mail_mail_send(self):
        """The production path: IrMailServer.send_email raises, mail.mail._send records the failure
        on the mail and swallows it, and the hook still reports."""
        mail = self.make_mail()
        self.env.flush_all()
        # Odoo skips sending while a test runs unless this is patched, as its own mail tests do.
        self.safe_patch_object(type(self.env["ir.mail_server"]), "_disable_send", return_value=False)
        self.safe_patch_object(
            type(self.env["ir.mail_server"]), "send_email", side_effect=OSError("Connection timed out")
        )
        mail._send(raise_exception=False)
        self.env.invalidate_all()
        self.assertEqual(mail.state, "exception")
        rows = self.mail_rows()
        self.assertEqual(len(rows), 1)
        self.assertIn("timeout", rows.frame)

    def test_failure_class_vocabulary(self):
        cases = {
            "535 5.7.8 Authentication failed": "auth",
            "SSL: CERTIFICATE_VERIFY_FAILED": "tls",
            "timed out": "timeout",
            "Connection refused": "connect",
            "421 try later": "temporary_4xx",
            "550 mailbox unavailable": "permanent_5xx",
            "something odd": "other",
            False: "other",
        }
        for text, label in cases.items():
            self.assertEqual(scrub.mail_failure_class(text), label, text)

    # Tests [@ANCHOR: hams_helpdesk:exc_mail_bounce]
    def test_bounce_processing_failure_is_reported_and_reraised(self):
        self.env.flush_all()
        # A bounce dict without the keys Odoo reads makes the real handler raise.
        # Not assertRaises: Odoo's wraps the block in a savepoint, and its rollback would also undo
        # the report, which is written on a cursor that joins this test's transaction.
        raised = False
        try:
            self.env["mail.thread"]._routing_handle_bounce(None, {self.VICTIM: 1})
        except KeyError as error:
            raised = True
            _logger.debug("the bounce handler failed as expected: %r", error)
        self.assertTrue(raised, "the failure must still propagate")
        self.env.invalidate_all()
        rows = self.mail_rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.exc_type, "builtins.KeyError")
        self.assertNotIn(self.VICTIM, rows.samples_json + rows.ticket_id.description)


def load_daemon_reporter():
    """The daemon-side helper, loaded from its file the way a daemon imports it."""
    path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "daemon", "exception_reporter.py")
    spec = importlib.util.spec_from_file_location("hams_exception_reporter_under_test", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install", "standard")
class TestDaemonFailureReports(ExceptionTicketCase):
    """``report_daemon_failure``: the Odoo end of the daemon / OnFailure reporter."""

    # Tests [@ANCHOR: hams_helpdesk:exc_report_daemon_failure]
    # Tests [@ANCHOR: hams_helpdesk:exc_daemon_unit_name]
    # Tests [@ANCHOR: hams_helpdesk:exc_daemon_tail]

    def as_reporter(self):
        uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_exception_reporter_service")
        return self.Fingerprint.with_user(uid)

    def daemon_rows(self):
        return self.Fingerprint.search([("source", "=", "daemon")])

    def test_unit_failure_makes_one_scrubbed_ticket(self):
        self.env.flush_all()
        tail = "\n".join(
            (
                "starting consumer",
                "Traceback (most recent call last):",
                SECRET_MESSAGE,
                "ValueError: bad value for k6bp@example.com",
            )
        )
        answer = self.as_reporter().report_daemon_failure(
            "ses-inbound-mail-ingest.service", "unit_failed", "exit-code", tail
        )
        self.env.invalidate_all()
        self.assertEqual(answer, {"recorded": True})
        rows = self.daemon_rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.module, "ses-inbound-mail-ingest.service")
        self.assertEqual(rows.route, "systemd:ses-inbound-mail-ingest.service")
        self.assertEqual(rows.ticket_id.ticket_type, "server_exception")
        stored = " ".join((rows.samples_json, rows.ticket_id.description, rows.ticket_id.name))
        for needle in ("k6bp@example.com", "203.0.113.9", "hunter2", "abcdef0123456789", "K6BP", "15551234567"):
            self.assertNotIn(needle, stored)
        self.assertIn("ValueError: <message withheld>", stored)

    def test_repeats_are_one_ticket(self):
        self.env.flush_all()
        service = self.as_reporter()
        service.report_daemon_failure("adif-processor.service", "unit_failed", "exit-code", "x")
        again = service.report_daemon_failure("adif-processor.service", "unit_failed", "exit-code", "x")
        self.env.invalidate_all()
        self.assertEqual(again, {"recorded": False})
        self.assertEqual(len(self.daemon_rows()), 1)

    def test_another_account_is_refused(self):
        with self.assertRaises(AccessError):
            self.Fingerprint.report_daemon_failure("adif-processor.service", "unit_failed", "exit-code", "x")
        self.assertFalse(self.daemon_rows())

    def test_bad_input_is_refused(self):
        service = self.as_reporter()
        for unit in ("", "../etc/passwd", "a b.service", "x" * 200, "k6bp@example.com"):
            with self.assertRaises(UserError, msg=unit):
                service.report_daemon_failure(unit, "unit_failed", "exit-code", "x")
        with self.assertRaises(UserError):
            service.report_daemon_failure("a.service", "something_else", "exit-code", "x")

    def test_instance_is_dropped_from_a_template_unit(self):
        self.assertEqual(scrub.daemon_unit_name("worker@k6bp.service"), "worker@.service")
        self.assertEqual(scrub.daemon_unit_name("plain.service"), "plain.service")
        self.assertIsNone(scrub.daemon_unit_name("-bad.service"))

    def test_tail_is_bounded_and_unknown_result_is_not_stored(self):
        huge = "\n".join(f"line {number} " + "y" * 500 for number in range(500))
        cut = scrub.scrub_daemon_tail(huge)
        self.assertLessEqual(len(cut.split("\n")), scrub.DAEMON_TAIL_LINES)
        self.assertLessEqual(len(cut), scrub.DAEMON_TAIL_LIMIT)
        self.env.flush_all()
        self.as_reporter().report_daemon_failure("a.service", "consumer_exception", "k6bp@example.com", huge)
        self.env.invalidate_all()
        rows = self.daemon_rows()
        self.assertIn("result:unknown", rows.frame)
        self.assertNotIn("example.com", rows.samples_json)


@tagged("post_install", "-at_install", "standard")
class TestDaemonReporterHelper(RealTransactionCase):
    """The daemon-side helper against the real JSON-2 endpoint with a real API key of the reporter
    service account: the path the OnFailure unit and a daemon's ``reporting_failures`` take."""

    # Tests [@ANCHOR: hams_helpdesk:daemon_post_report]
    # Tests [@ANCHOR: hams_helpdesk:daemon_load_key]
    # Tests [@ANCHOR: hams_helpdesk:daemon_report_unit]
    # Tests [@ANCHOR: hams_helpdesk:daemon_report_exception]
    # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures]
    # Tests [@ANCHOR: hams_helpdesk:daemon_frames_only]
    # Tests [@ANCHOR: hams_helpdesk:daemon_journal_tail]
    # Tests [@ANCHOR: hams_helpdesk:daemon_unit_result]

    def setUp(self):
        super().setUp()
        reporter._GATE.clear()
        reporter._RECENT_WRITES.clear()
        self.addCleanup(reporter._GATE.clear)
        self.addCleanup(reporter._RECENT_WRITES.clear)
        self.helper = load_daemon_reporter()
        self.helper._LAST_REPORT.clear()
        uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_exception_reporter_service")
        key = self.env["res.users.apikeys"].with_user(uid)._generate(None, "daemon failure test", fields.Datetime.now() + timedelta(days=1))
        self.env.cr.commit()
        handle = tempfile.NamedTemporaryFile("w", suffix=".env", delete=False)
        self.addCleanup(os.unlink, handle.name)
        with handle:
            handle.write(f"ODOO_RPC_LOGIN=exception_reporter_service_internal\nODOO_RPC_KEY={key}\n")
        os.chmod(handle.name, 0o600)
        self.key_file = handle.name
        self.options = {"url": self.base_url(), "key_file": self.key_file, "database": self.env.cr.dbname}

    def rows(self):
        self.env.cr.rollback()
        self.env.invalidate_all()
        return self.env["hams_helpdesk.exception.fingerprint"].search([("source", "=", "daemon")])

    def test_report_unit_reaches_a_ticket(self):
        answer = self.helper.post_report(
            {"unit": "dx-firehose.service", "kind": "unit_failed", "result": "signal", "journal_tail": "boom"},
            **self.options,
        )
        self.assertEqual(answer, {"recorded": True})
        rows = self.rows()
        self.assertEqual(rows.module, "dx-firehose.service")
        self.assertEqual(rows.ticket_id.ticket_type, "server_exception")

    def test_reporting_failures_reports_then_raises_the_same_exception(self):
        secret = "bad value for k6bp@example.com"
        with self.assertRaises(KeyError) as caught:
            with self.helper.reporting_failures("club-crawl.service", **self.options):
                raise KeyError(secret)
        self.assertEqual(caught.exception.args, (secret,))
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.exc_type, "daemon.consumer_exception")
        self.assertNotIn("example.com", rows.samples_json)
        self.assertIn("KeyError", rows.samples_json)

    def test_reporting_failures_does_not_report_a_stop_signal(self):
        with self.assertRaises(SystemExit):
            with self.helper.reporting_failures("club-crawl.service", **self.options):
                raise SystemExit(0)
        self.assertFalse(self.rows())

    def test_a_failed_report_never_hides_the_original_exception(self):
        broken = dict(self.options, key_file="/nonexistent/key.env")
        with self.assertRaises(ZeroDivisionError):
            with self.helper.reporting_failures("club-crawl.service", **broken):
                1 / 0  # audit-ignore-division: raises on purpose
        self.assertFalse(self.rows())

    def test_a_wrong_key_is_refused_without_leaking_it(self):
        with open(self.key_file, "w", encoding="utf-8") as handle:
            handle.write("ODOO_RPC_KEY=not-a-real-key\n")
        with self.assertRaises(self.helper.ReportError) as caught:
            self.helper.post_report({"unit": "a.service", "kind": "unit_failed"}, **self.options)
        self.assertNotIn("not-a-real-key", str(caught.exception))
        self.assertNotIn(self.options["url"], str(caught.exception))

    def test_missing_key_file_fails_fast(self):
        with self.assertRaises(self.helper.ReportError):
            self.helper.load_key("/nonexistent/key.env")

    def test_reports_are_limited_to_one_a_minute_per_name(self):
        exc = TypeError("x")
        self.assertIsNotNone(self.helper.report_exception("limit.service", exc, **self.options))
        self.assertIsNone(self.helper.report_exception("limit.service", exc, **self.options))

    def test_the_key_file_is_the_reporters_own_never_the_daemons_odoo_key_file(self):
        # A daemon's unit sets ODOO_KEY_FILE to the daemon's own key, which the endpoint refuses.
        seen = []

        def fake_load_key(path):
            seen.append(path)
            raise self.helper.ReportError("stop here")

        saved = {name: os.environ.get(name) for name in ("ODOO_KEY_FILE", "EXCEPTION_REPORTER_KEY_FILE")}

        def restore():
            for name, value in saved.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value

        self.addCleanup(restore)
        self.safe_patch_object(self.helper, "load_key", new=fake_load_key)
        os.environ["ODOO_KEY_FILE"] = "/opt/hams/etc/keys/some_daemon.env"
        os.environ.pop("EXCEPTION_REPORTER_KEY_FILE", None)
        with self.assertRaises(self.helper.ReportError):
            self.helper.post_report({"unit": "a.service", "kind": "unit_failed"}, url="http://odoo.invalid:1")
        os.environ["EXCEPTION_REPORTER_KEY_FILE"] = "/run/other/key.env"
        with self.assertRaises(self.helper.ReportError):
            self.helper.post_report({"unit": "a.service", "kind": "unit_failed"}, url="http://odoo.invalid:1")
        self.assertEqual(seen, ["/opt/hams/etc/keys/exception_reporter.env", "/run/other/key.env"])

    def test_main_rejects_other_arguments(self):
        self.assertEqual(self.helper.main(["nonsense"]), 2)

    def test_frames_only_carries_no_message(self):
        try:
            raise_type_error("secret k6bp@example.com")
        except TypeError as exc:
            text = self.helper.frames_only(exc)
        self.assertNotIn("example.com", text)
        self.assertIn("TypeError", text)
        self.assertIn("raise_type_error", text)

    def test_journal_and_result_readers_return_text_for_a_real_unit(self):
        # Read-only calls against this machine's own systemd: a real unit name is not needed, the
        # readers must give a string either way and never raise.
        self.assertIsInstance(self.helper.journal_tail("no-such-unit-for-test.service"), str)
        self.assertIsInstance(self.helper.unit_result("no-such-unit-for-test.service"), str)


@tagged("post_install", "-at_install", "standard")
class TestExceptionHooks(RealTransactionCase):
    """Real connections and real commits: the hooks run the way they do in production, in the
    request's own transaction, and the report has to survive that transaction's rollback."""

    # Tests [@ANCHOR: hams_helpdesk:exc_report_request_exception]
    # Tests [@ANCHOR: hams_helpdesk:exc_ir_http_handle_error]
    # Tests [@ANCHOR: hams_helpdesk:exc_ir_http_pre_dispatch]
    # Tests [@ANCHOR: hams_helpdesk:exc_ir_cron_callback]
    # Tests [@ANCHOR: hams_helpdesk:exc_write_report]

    def setUp(self):
        super().setUp()
        reporter._GATE.clear()
        reporter._RECENT_WRITES.clear()
        self.addCleanup(reporter._GATE.clear)
        self.addCleanup(reporter._RECENT_WRITES.clear)
        self.Fingerprint = self.env["hams_helpdesk.exception.fingerprint"]

    def rows(self, domain=()):
        self.env.cr.rollback()  # a fresh snapshot: the reporter committed on its own connection
        self.env.invalidate_all()
        return self.Fingerprint.search(list(domain))

    def test_rpc_error_from_the_web_client_makes_exactly_one_ticket(self):
        self.authenticate("admin", "admin")
        route = "/web/dataset/call_kw/res.partner.no_such_method"
        params = {"model": "res.partner", "method": "no_such_method", "args": [[]], "kwargs": {}}
        for _ in range(3):
            response = self.opener.post(
                self.base_url() + route,
                json={"id": 0, "jsonrpc": "2.0", "method": "call", "params": params},
            )
            self.assertIn("error", response.json())
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.exc_type, "builtins.AttributeError")
        self.assertEqual(rows.source, "rpc")
        self.assertEqual(rows.route, route)
        self.assertEqual(rows.occurrence_count, 1)
        self.assertEqual(rows.ticket_id.ticket_type, "server_exception")
        self.assertEqual(rows.last_user_id, self.env.ref("base.user_admin").id)

    def test_not_found_page_makes_no_ticket(self):
        self.url_open("/this/page/does/not/exist-12345")
        self.assertFalse(self.rows())

    def test_failing_cron_job_is_reported_and_still_raises(self):
        cron = self.env["ir.cron"].create(
            {
                "name": "Exception ticket test job",
                "model_id": self.env.ref("base.model_res_partner").id,
                "state": "code",
                "code": "raise_cron_failure = {}['missing']",
                "interval_number": 1,
                "interval_type": "days",
                "active": False,
            }
        )
        self.env.cr.commit()
        # Not assertRaises: Odoo's wraps the block in a savepoint, which the job's own rollback
        # (real here, as in production) destroys.
        raised = False
        try:
            cron._callback(cron.cron_name, cron.ir_actions_server_id.id)
        except ValueError as error:
            raised = True
            _logger.debug("the job failed as expected: %r", error)
        self.assertTrue(raised, "the cron failure must still propagate")
        rows = self.rows([("source", "=", "cron")])
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.route, "cron: Exception ticket test job")
        # safe_eval wraps every server-action error in a ValueError; the ticket names the real one.
        self.assertEqual(rows.exc_type, "builtins.KeyError")

    def test_report_survives_the_rollback_of_the_failed_transaction(self):
        exc = TypeError("bad")
        with self.assertRaises(psycopg2.errors.DivisionByZero):
            with mute_logger("odoo.sql_db"):
                self.env.cr.execute("SELECT 1/0")  # audit-ignore-sql: aborts the transaction on purpose
        # The transaction is now aborted, as a failed request's would be.
        written = reporter.report_exception(
            self.env.registry, exc, "rpc", uid=2, logged_in=True, path="/web/dataset/call_kw/a.b"
        )
        self.assertTrue(written)
        self.assertEqual(len(self.rows()), 1)
