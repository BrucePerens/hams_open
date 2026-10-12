# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Automatic tickets for unhandled server exceptions (models/exception_fingerprint.py).

The reporter is exercised through its real entry points: ``report_exception`` for the
classification, scrubbing, de-duplication and storm tests, ``/web/dataset/call_kw`` over HTTP for the
ir.http hook, and ``ir.cron._callback`` for the cron hook.
"""
import json
import logging

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
