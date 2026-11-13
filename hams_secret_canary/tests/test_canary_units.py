# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free unit tests of the secret canary: the detector, the WSGI scan, the alert backends and the model's own
logic against fakes. Added to kill the mutants the database-level tests left alive (mutation run 2026-11-13); every
assertion pins an exact value, a call argument or a branch."""
import hashlib
import logging
import os
import stat
import string
import tempfile
import shutil
from contextlib import contextmanager
from types import SimpleNamespace

from odoo import models
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_secret_canary import alerts, detector, hooks, wsgi_scan
from odoo.addons.hams_secret_canary.models import ir_http, secret_canary

SALT = "0123456789abcdef0123456789abcdef"
TOKEN = "whsec_Zk39dQ7mPq2LxV8nTb4RcY6a"
OTHER = "decoy_AbCdEfGhIjKlMnOpQrStUvWx"


def _digest(token, salt=SALT):
    return hashlib.sha256((salt + token).encode("utf-8")).hexdigest()


def _table(**labels):
    return {_digest(value): (label, "secret") for label, value in labels.items()}


@tagged("post_install", "-at_install")
class TestDetectorUnits(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:fingerprint]
    # Tests [@ANCHOR: hams_secret_canary:new_salt]
    # Tests [@ANCHOR: hams_secret_canary:make_canary]
    # Tests [@ANCHOR: hams_secret_canary:is_token]
    # Tests [@ANCHOR: hams_secret_canary:scannable_response]
    # Tests [@ANCHOR: hams_secret_canary:candidates]
    # Tests [@ANCHOR: hams_secret_canary:find_matches]
    # Tests [@ANCHOR: hams_secret_canary:redact]

    def test_fingerprint_is_the_salted_sha256(self):
        self.assertEqual(detector.fingerprint("s", "t"), hashlib.sha256(b"st").hexdigest())
        self.assertEqual(detector.fingerprint(SALT, TOKEN), _digest(TOKEN))
        self.assertNotEqual(detector.fingerprint("other", TOKEN), _digest(TOKEN))
        self.assertEqual(detector.fingerprint("s", "é"), hashlib.sha256("sé".encode("utf-8")).hexdigest())

    def test_new_salt_is_32_hex_characters_and_fresh(self):
        first, second = detector.new_salt(), detector.new_salt()
        self.assertRegex(first, r"^[0-9a-f]{32}$")
        self.assertNotEqual(first, second)

    def test_make_canary_follows_each_shape(self):
        alnum = set(string.ascii_letters + string.digits)
        stripe = detector.make_canary("stripe_secret_key")
        self.assertTrue(stripe.startswith("sk_live_"))
        self.assertEqual(len(stripe), 8 + 24)
        self.assertLessEqual(set(stripe[8:]), alnum)
        s3 = detector.make_canary("s3_secret_access_key")
        self.assertEqual(len(s3), 40)
        self.assertLessEqual(set(s3), alnum | set("/+"))
        cloudflare = detector.make_canary("cloudflare_api_token")
        self.assertEqual(len(cloudflare), 40)
        self.assertLessEqual(set(cloudflare), alnum | set("-_"))
        self.assertNotEqual(detector.make_canary("stripe_secret_key"), detector.make_canary("stripe_secret_key"))
        with self.assertRaises(KeyError):
            detector.make_canary("no_such_label")
        self.assertEqual(set(detector.CANARY_SHAPES), {"stripe_secret_key", "s3_secret_access_key", "cloudflare_api_token"})

    def test_is_token_boundaries(self):
        self.assertTrue(detector.is_token("a" * 16))
        self.assertTrue(detector.is_token("a" * 512))
        self.assertFalse(detector.is_token("a" * 15))
        self.assertFalse(detector.is_token("a" * 513))
        self.assertTrue(detector.is_token("A0_-+/." + "b" * 10))
        self.assertTrue(detector.is_token("." + "b" * 15), "a leading dot is allowed")
        self.assertFalse(detector.is_token("b" * 15 + "."), "a trailing dot is not")
        for junk in ("", None, "a" * 16 + " ", " " + "a" * 16, "a" * 8 + " " + "a" * 8, "a" * 16 + "\n", "a" * 15 + "!",
                     "a" * 15 + "é"):
            self.assertFalse(detector.is_token(junk), repr(junk))
        self.assertIs(detector.is_token(""), False)
        self.assertIs(detector.is_token("a" * 16), True)

    def test_scannable_response(self):
        def response(mimetype, direct_passthrough=False, is_sequence=True):
            return SimpleNamespace(mimetype=mimetype, direct_passthrough=direct_passthrough, is_sequence=is_sequence)
        for mimetype in ("text/html", "text/plain", "text/", "application/json", "application/javascript",
                         "application/xml", "application/xhtml+xml"):
            self.assertTrue(detector.scannable_response(response(mimetype)), mimetype)
        for mimetype in ("image/png", "application/octet-stream", "application/pdf", "", None, "texts/html",
                         "application/jsonx", "xtext/html"):
            self.assertFalse(detector.scannable_response(response(mimetype)), repr(mimetype))
        self.assertIs(detector.scannable_response(response("text/html", direct_passthrough=True)), False)
        self.assertIs(detector.scannable_response(response("text/html", is_sequence=False)), False)
        self.assertIs(detector.scannable_response(response("text/html", True, False)), False)
        self.assertIs(detector.scannable_response(response("text/html")), True)
        self.assertIs(detector.scannable_response(response("image/png")), False)

    def test_candidates(self):
        x, y = "x" * 16, "y" * 20
        token = "..." + x + "/" + y + "..."
        self.assertEqual(list(detector._candidates(token)),
                         [token, x + "/" + y, "..." + x, y + "..."], "the token, then the dotless form, then each piece")
        self.assertEqual(list(detector._candidates("a" * 16)), ["a" * 16])
        self.assertEqual(list(detector._candidates("." + "a" * 16 + ".")), ["." + "a" * 16 + ".", "a" * 16])
        self.assertEqual(list(detector._candidates("a" * 16 + "/" + "b" * 15)), ["a" * 16 + "/" + "b" * 15, "a" * 16],
                         "a 15-character piece is too short")
        self.assertEqual(list(detector._candidates("/" + "c" * 16)), ["/" + "c" * 16, "c" * 16], "16 is enough")
        self.assertEqual(list(detector._candidates("a" * 16 + "/" + "a" * 16)), ["a" * 16 + "/" + "a" * 16, "a" * 16],
                         "an equal piece is yielded once")
        self.assertEqual(list(detector._candidates("..." + "z" * 16)), ["..." + "z" * 16, "z" * 16])

    def test_find_matches_each_once_in_order_of_first_appearance(self):
        table = _table(first=TOKEN, second=OTHER)
        text = "x %s y %s z %s" % (OTHER, TOKEN, OTHER)
        self.assertEqual(detector.find_matches(text, SALT, table), [("second", "secret"), ("first", "secret")])
        self.assertEqual(detector.find_matches("%s %s" % (TOKEN, OTHER), SALT, table), [("first", "secret"), ("second", "secret")])
        self.assertEqual(detector.find_matches("nothing here at all", SALT, table), [])
        self.assertEqual(detector.find_matches(TOKEN, SALT, {}), [])
        self.assertEqual(detector.find_matches(TOKEN, "wrong salt", table), [])

    def test_find_matches_forms_a_secret_can_take(self):
        table = {_digest(TOKEN): ["lst", "canary"]}
        self.assertEqual(detector.find_matches("a/b/%s/c" % TOKEN, SALT, table), [("lst", "canary")], "in a path")
        self.assertEqual(detector.find_matches("%s." % TOKEN, SALT, table), [("lst", "canary")], "a sentence's full stop")
        self.assertEqual(detector.find_matches("...%s..." % TOKEN, SALT, table), [("lst", "canary")])
        self.assertEqual(detector.find_matches('{"k":"%s"}' % TOKEN, SALT, table), [("lst", "canary")])
        self.assertEqual(detector.find_matches("%sX" % TOKEN, SALT, table), [], "a longer token is another token")
        self.assertEqual(detector.find_matches(TOKEN[:-1], SALT, table), [])
        self.assertEqual(detector.find_matches("/" + TOKEN, SALT, table), [("lst", "canary")])

    def test_find_matches_entries_may_be_lists_and_are_returned_as_tuples(self):
        found = detector.find_matches(TOKEN, SALT, {_digest(TOKEN): ["lbl", "kind"]})
        self.assertEqual(found, [("lbl", "kind")])
        self.assertIsInstance(found[0], tuple)

    def test_find_matches_orders_by_the_position_of_the_token_in_the_text(self):
        # a second secret that is a path piece of an earlier token is still ordered by its token's position
        pre = "p" * 16
        table = _table(path_piece=OTHER, plain=TOKEN)
        text = "%s/%s then %s" % (pre, OTHER, TOKEN)
        self.assertEqual(detector.find_matches(text, SALT, table), [("path_piece", "secret"), ("plain", "secret")])
        text = "%s then %s/%s" % (TOKEN, pre, OTHER)
        self.assertEqual(detector.find_matches(text, SALT, table), [("plain", "secret"), ("path_piece", "secret")])

    def test_find_matches_token_length_limits(self):
        long_secret = "L" * 512
        self.assertEqual(detector.find_matches(" %s " % long_secret, SALT, _table(long=long_secret)), [("long", "secret")])
        too_long = "L" * 513
        self.assertEqual(detector.find_matches(" %s " % too_long, SALT, _table(too_long=too_long)), [])
        short = "S" * 16
        self.assertEqual(detector.find_matches(" %s " % short, SALT, _table(short=short)), [("short", "secret")])

    def test_redact(self):
        table = _table(first=TOKEN, second=OTHER)
        self.assertEqual(detector.redact("a %s b" % TOKEN, SALT, table), "a [REDACTED:first] b")
        self.assertEqual(detector.redact("%s,%s" % (TOKEN, OTHER), SALT, table), "[REDACTED:first],[REDACTED:second]")
        self.assertEqual(detector.redact("%s." % TOKEN, SALT, table), "[REDACTED:first].", "the full stop stays")
        self.assertEqual(detector.redact("/api/%s/x" % TOKEN, SALT, table), "/api/[REDACTED:first]/x")
        self.assertEqual(detector.redact("/api/%s/x" % ("p" * 16 + "/" + TOKEN), SALT, table),
                         "/api/" + "p" * 16 + "/[REDACTED:first]/x")
        self.assertEqual(detector.redact("nothing to see here", SALT, table), "nothing to see here")
        self.assertEqual(detector.redact(TOKEN, SALT, {}), TOKEN)
        self.assertEqual(detector.redact("%s %s" % (TOKEN, TOKEN), SALT, table), "[REDACTED:first] [REDACTED:first]")
        self.assertEqual(detector.redact("", SALT, table), "")
        self.assertNotIn(TOKEN, detector.redact("k=%s&j=%s" % (TOKEN, OTHER), SALT, table))
        self.assertEqual(detector.redact(TOKEN, SALT, {_digest(TOKEN): ["lst", "k"]}), "[REDACTED:lst]")


class _Body:
    """A WSGI body that yields chunks and records that it was closed."""

    def __init__(self, chunks):
        self.chunks = chunks
        self.closed = False

    def __iter__(self):
        return iter(self.chunks)

    def close(self):
        self.closed = True


@tagged("post_install", "-at_install")
class TestWsgiScanUnits(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:mark_scanned]
    # Tests [@ANCHOR: hams_secret_canary:scan_wsgi]
    # Tests [@ANCHOR: hams_secret_canary:pass_through]
    # Tests [@ANCHOR: hams_secret_canary:scan_chunks]

    def _serve(self, headers, chunks, environ=None, scan_in_app=False, limit=None):
        scans, started = [], []
        body = _Body(chunks)

        def app(environ, start_response):
            start_response("200 OK", headers)
            if scan_in_app:
                wsgi_scan.mark_scanned()
            return body

        if limit is not None:
            self.safe_patch_object(detector, "MAX_SCAN_BYTES", new=limit)
        wrapped = wsgi_scan.scan_wsgi(app, lambda text, path: scans.append((text, path)))
        result = wrapped({"PATH_INFO": "/p"} if environ is None else environ, lambda *a: started.append(a))
        return scans, started, body, result

    def test_is_text(self):
        # Tests [@ANCHOR: hams_secret_canary:is_text]
        for value in ("text/html", "TEXT/PLAIN; charset=utf-8", " text/css ;x=y", "application/json; charset=utf-8",
                      "Application/XML"):
            self.assertTrue(wsgi_scan._is_text(value), value)
        for value in ("image/png", "application/octet-stream", "", "texts/html; text/html", "x/text/html"):
            self.assertFalse(wsgi_scan._is_text(value), value)

    def test_text_is_passed_through_unchanged_and_scanned_once_after_it_is_sent(self):
        scans, started, body, result = self._serve([("Content-Type", "text/plain")], [b"ab", b"cd", b"ef"])
        self.assertEqual(scans, [], "nothing is scanned before the body is consumed")
        self.assertEqual(list(result), [b"ab", b"cd", b"ef"])
        self.assertEqual(scans, [("abcdef", "/p")])
        self.assertEqual(started, [("200 OK", [("Content-Type", "text/plain")])], "the start_response call is unchanged")

    def test_start_response_gets_exc_info_only_when_there_is_one(self):
        calls = []
        wrapped = wsgi_scan.scan_wsgi(lambda environ, start_response: (start_response("500", [], ("t", "v", "tb")),
                                                                      start_response("200", []), [])[2],
                                      lambda *a: None)
        wrapped({}, lambda *a: calls.append(a))
        self.assertEqual(calls, [("500", [], ("t", "v", "tb")), ("200", [])])

    def test_the_content_type_header_is_found_case_insensitively(self):
        scans, _started, _body, result = self._serve([("content-type", "text/plain")], [b"x" * 20])
        list(result)
        self.assertEqual(len(scans), 1)
        scans, _started, _body, result = self._serve([("X-Other", "text/plain"), ("CONTENT-TYPE", "text/html")], [b"y"])
        list(result)
        self.assertEqual(scans, [("y", "/p")])

    def test_no_scan_for_binary_for_no_content_type_or_when_post_dispatch_already_scanned(self):
        for headers, kwargs in (([("Content-Type", "image/png")], {}), ([], {}), ([("X-Other", "text/plain")], {}),
                                ([("Content-Type", "text/plain")], {"scan_in_app": True})):
            scans, _started, body, result = self._serve(headers, [b"zz"], **kwargs)
            self.assertIs(result, body, "the application's own body is returned untouched")
            self.assertEqual(list(result), [b"zz"])
            self.assertEqual(scans, [], str(headers))

    def test_the_scanned_flag_is_reset_for_every_request(self):
        wsgi_scan.mark_scanned()  # left over from an earlier request on this thread
        scans, _started, _body, result = self._serve([("Content-Type", "text/plain")], [b"hello"])
        list(result)
        self.assertEqual(scans, [("hello", "/p")])

    def test_the_path_defaults_to_empty(self):
        scans, _started, _body, result = self._serve([("Content-Type", "text/plain")], [b"q"], environ={})
        list(result)
        self.assertEqual(scans, [("q", "")])

    def test_a_method_style_application_gets_its_self(self):
        seen = []

        class Application:
            def __call__(self, environ, start_response):
                seen.append(self)
                start_response("200", [("Content-Type", "text/plain")])
                return [b"m"]
        scans = []
        wrapped = wsgi_scan.scan_wsgi(Application.__call__, lambda text, path: scans.append((text, path)))
        instance = Application()
        list(wrapped(instance, {"PATH_INFO": "/m"}, lambda *a: None))
        self.assertEqual(seen, [instance])
        self.assertEqual(scans, [("m", "/m")])

    def test_only_the_first_bytes_are_scanned_and_the_client_still_gets_everything(self):
        scans, _started, _body, result = self._serve([("Content-Type", "text/plain")], [b"abcd", b"efgh", b"ijkl"], limit=6)
        self.assertEqual(list(result), [b"abcd", b"efgh", b"ijkl"])
        self.assertEqual(scans, [("abcdef", "/p")])
        scans, _started, _body, result = self._serve([("Content-Type", "text/plain")], [b"abcdef", b"gh"], limit=6)
        list(result)
        self.assertEqual(scans, [("abcdef", "/p")], "a body of exactly the limit is scanned whole")
        scans, _started, _body, result = self._serve([("Content-Type", "text/plain")], [b"abcde", b"fg"], limit=6)
        list(result)
        self.assertEqual(scans, [("abcdef", "/p")])

    def test_undecodable_bytes_are_replaced_not_fatal(self):
        scans, _started, _body, result = self._serve([("Content-Type", "text/plain")], [b"a\xffb"])
        list(result)
        self.assertEqual(scans, [("a�b", "/p")])

    def test_closing_the_result_closes_the_applications_body(self):
        _scans, _started, body, result = self._serve([("Content-Type", "text/plain")], [b"abc"])
        self.assertFalse(body.closed)
        result.close()
        self.assertTrue(body.closed)

    def test_mark_scanned_sets_the_thread_flag(self):
        wsgi_scan._served.scanned = False
        wsgi_scan.mark_scanned()
        self.assertIs(wsgi_scan._served.scanned, True)


class _Cursor:
    def savepoint(self):
        return self

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


class _ServiceEnv(dict):
    """A service environment that records the records created through it."""

    def __init__(self):
        super().__init__()
        self.created = []
        self.email = "admin@example.test"

    def __missing__(self, model):
        created = self.created

        class Model:
            def create(self, values):
                created.append((model, values))
        return Model()

    def ref(self, xml_id):
        assert xml_id == "base.user_admin"
        return SimpleNamespace(partner_id=SimpleNamespace(email=self.email))


class _Env(dict):
    def __init__(self, services=None, pager=False):
        super().__init__()
        self.cr = _Cursor()
        self.service_calls = []
        self.service = _ServiceEnv()
        self.pager_calls = []
        outer = self

        class Utils:
            def _get_service_env(self, xml_id):
                outer.service_calls.append(("env", xml_id))
                return outer.service

            def _get_service_uid(self, xml_id):
                outer.service_calls.append(("uid", xml_id))
                return 77
        self["zero_sudo.security.utils"] = Utils()
        if pager:
            class Incident:
                def with_user(self, uid):
                    outer.pager_calls.append(("user", uid))
                    return self

                def report_incident(self, values):
                    outer.pager_calls.append(("report", values))
            self["pager.incident"] = Incident()


@tagged("post_install", "-at_install")
class TestAlertBackends(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:register_backend]
    # Tests [@ANCHOR: hams_secret_canary:raise_alarm]
    # Tests [@ANCHOR: hams_secret_canary:logging_backend]
    # Tests [@ANCHOR: hams_secret_canary:record_backend]
    # Tests [@ANCHOR: hams_secret_canary:mail_backend]
    # Tests [@ANCHOR: hams_secret_canary:pager_backend]

    ALARM = alerts.Alarm("stripe_key", "canary", "response", "/the/path")

    def setUp(self):
        super().setUp()
        saved = dict(alerts.BACKENDS)
        self.addCleanup(lambda: (alerts.BACKENDS.clear(), alerts.BACKENDS.update(saved)))

    def test_the_four_standard_backends_are_registered_in_order(self):
        self.assertEqual(list(alerts.BACKENDS), ["logging", "record", "mail", "pager"])
        self.assertIs(alerts.BACKENDS["logging"], alerts.logging_backend)
        self.assertIs(alerts.BACKENDS["record"], alerts.record_backend)
        self.assertIs(alerts.BACKENDS["mail"], alerts.mail_backend)
        self.assertIs(alerts.BACKENDS["pager"], alerts.pager_backend)

    def test_an_alarm_is_immutable_and_carries_four_fields(self):
        alarm = alerts.Alarm("l", "k", "log", "w")
        self.assertEqual((alarm.label, alarm.kind, alarm.source, alarm.where), ("l", "k", "log", "w"))
        with self.assertRaises(AttributeError):
            alarm.label = "x"

    def test_register_backend_returns_the_function_and_replaces_one_of_that_name(self):
        def first(env, alarm):
            pass

        def second(env, alarm):
            pass
        self.assertIs(alerts.register_backend("extra")(first), first)
        self.assertIs(alerts.BACKENDS["extra"], first)
        alerts.register_backend("extra")(second)
        self.assertIs(alerts.BACKENDS["extra"], second)
        self.assertEqual(list(alerts.BACKENDS).count("extra"), 1)

    def test_raise_alarm_runs_every_backend_in_order_with_the_env_and_alarm(self):
        alerts.BACKENDS.clear()
        order = []
        alerts.register_backend("a")(lambda env, alarm: order.append(("a", env, alarm)))
        alerts.register_backend("b")(lambda env, alarm: order.append(("b", env, alarm)))
        alerts.raise_alarm("ENV", self.ALARM)
        self.assertEqual(order, [("a", "ENV", self.ALARM), ("b", "ENV", self.ALARM)])

    def test_raise_alarm_propagates_a_failure_and_is_not_confused_by_a_backend_that_registers_another(self):
        alerts.BACKENDS.clear()
        ran = []

        def adds_one(env, alarm):
            alerts.register_backend("late")(lambda e, a: ran.append("late"))
            ran.append("adds_one")
        alerts.register_backend("adds")(adds_one)
        alerts.raise_alarm(None, self.ALARM)
        self.assertEqual(ran, ["adds_one"], "the backends of this alarm are those registered when it began")

        def boom(env, alarm):
            raise RuntimeError("backend failed")
        alerts.BACKENDS.clear()
        alerts.register_backend("boom")(boom)
        with self.assertRaises(RuntimeError):
            alerts.raise_alarm(None, self.ALARM)

    def test_the_logging_backend_writes_one_critical_line_and_works_without_an_env(self):
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
            alerts.logging_backend(None, self.ALARM)
        self.assertEqual(len(logs.records), 1)
        self.assertEqual(logs.records[0].levelno, logging.CRITICAL)
        self.assertEqual(logs.records[0].getMessage(), "SECRET CANARY TRIPPED: canary stripe_key seen in response (/the/path)")

    def test_the_record_backend_writes_a_security_log_row_as_the_facility_account(self):
        env = _Env()
        self.assertIsNone(alerts.record_backend(env, self.ALARM))
        self.assertEqual(env.service_calls, [("env", "zero_sudo.odoo_facility_service_internal")])
        self.assertEqual(env.service.created, [("zero_sudo.security.log", {
            "reason": "secret_canary", "login": "stripe_key", "user_agent": "response: /the/path"})])
        self.assertIsNone(alerts.record_backend(None, self.ALARM))

    def test_the_mail_backend_queues_one_mail_to_the_administrator(self):
        env = _Env()
        alerts.mail_backend(env, self.ALARM)
        self.assertEqual(env.service_calls, [("env", "zero_sudo.mail_service_internal")])
        self.assertEqual(env.service.created, [("mail.mail", {
            "subject": "Secret canary tripped: stripe_key",
            "body_html": "<p>A canary (stripe_key) was seen in response: /the/path.</p>"
                         "<p>This detects a leak after it happened.</p>",
            "email_to": "admin@example.test",
            "auto_delete": False})])
        self.assertIsNone(alerts.mail_backend(None, self.ALARM))

    def test_the_mail_backend_without_an_address_logs_and_queues_nothing(self):
        for email in (False, "", None):
            env = _Env()
            env.service.email = email
            with self.assertLogs("hams_secret_canary.alarm", "WARNING") as logs:
                self.assertIsNone(alerts.mail_backend(env, self.ALARM))
            self.assertEqual(env.service.created, [])
            self.assertEqual(logs.records[0].getMessage(),
                             "Secret canary alarm for stripe_key: the administrator has no email address, no mail queued.")

    def test_the_pager_backend_opens_a_critical_incident_as_the_pager_service(self):
        env = _Env(pager=True)
        self.safe_patch_object(alerts.fields.Datetime, "now", new=lambda: "2026-11-13 10:00:00")
        self.assertIsNone(alerts.pager_backend(env, self.ALARM))
        self.assertEqual(env.service_calls, [("uid", "pager_duty.user_pager_service_internal")])
        self.assertEqual(env.pager_calls, [("user", 77), ("report", {
            "source": "Secret canary: stripe_key", "severity": "critical",
            "description": "A canary (stripe_key) was seen in response: /the/path, 2026-11-13 10:00:00 UTC."})])
        for nothing in (_Env(pager=False), None):
            with self.assertNoLogs("hams_secret_canary.alarm", "WARNING"):
                self.assertIsNone(alerts.pager_backend(nothing, self.ALARM))
        self.assertEqual(env.pager_calls and len(env.pager_calls), 2)

    def test_a_failing_pager_never_reaches_the_response_and_never_names_the_cause_beyond_its_type(self):
        env = _Env(pager=True)

        def boom(values):
            raise ValueError("secret-looking detail")
        env["pager.incident"].report_incident = boom
        with self.assertLogs("hams_secret_canary.alarm", "WARNING") as logs:
            alerts.pager_backend(env, self.ALARM)
        self.assertEqual(logs.records[0].getMessage(),
                         "Secret canary alarm for stripe_key: the pager incident was not opened (ValueError).")
        self.assertNotIn("secret-looking", logs.output[0])


class _Kv:
    """The key-value store behind `zero_sudo.security.utils._get_kv/_set_kv`."""

    def __init__(self, raw=None):
        self.raw = raw
        self.reads = 0
        self.writes = []

    def _get_kv(self, key):
        assert key == "hams_secret_canary.state"
        self.reads += 1
        return self.raw

    def _set_kv(self, key, value):
        assert key == "hams_secret_canary.state"
        self.writes.append(value)
        self.raw = value


class _Params:
    def __init__(self, values=None):
        self.values = dict(values or {})
        self.sets = []

    def get_param(self, key):
        return self.values.get(key)

    def set_param(self, key, value):
        self.sets.append((key, value))
        self.values[key] = value


class _ModelEnv(dict):
    def __init__(self, kv=None, params=None, dbname="db1"):
        super().__init__()
        self.kv = kv or _Kv()
        self.params = params or _Params()
        self.cr = SimpleNamespace(dbname=dbname)
        self["zero_sudo.security.utils"] = self.kv
        self["ir.config_parameter"] = self.params


def _canary(env):
    record = object.__new__(secret_canary.SecretCanary)
    record.env = env
    return record


class _Clock:
    now = 1000.0

    def monotonic(self):
        return self.now


@tagged("post_install", "-at_install")
class TestCanaryModelUnits(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:load_state]
    # Tests [@ANCHOR: hams_secret_canary:store_state]
    # Tests [@ANCHOR: hams_secret_canary:register_secret]
    # Tests [@ANCHOR: hams_secret_canary:plant_canaries]
    # Tests [@ANCHOR: hams_secret_canary:plant_decoy_file_and_env]
    # Tests [@ANCHOR: hams_secret_canary:scan_text]
    # Tests [@ANCHOR: hams_secret_canary:register_hook]

    def setUp(self):
        super().setUp()
        saved = dict(secret_canary._tables)
        secret_canary._tables.clear()
        self.addCleanup(lambda: (secret_canary._tables.clear(), secret_canary._tables.update(saved)))
        self.clock = _Clock()
        self.safe_patch_object(secret_canary, "time", new=self.clock)
        self.alarms = []
        self.safe_patch_object(alerts, "raise_alarm", new=lambda env, alarm: self.alarms.append((env, alarm)))

    def test_load_state_of_an_empty_store_is_no_salt_and_no_table(self):
        env = _ModelEnv()
        self.assertEqual(_canary(env)._load_state(), (None, {}))
        self.assertIn("db1", secret_canary._tables)

    def test_load_state_reads_the_store_converts_entries_to_tuples_and_caches_for_thirty_seconds(self):
        raw = '{"salt": "%s", "table": {"d1": ["lbl", "secret"]}}' % SALT
        env = _ModelEnv(kv=_Kv(raw))
        record = _canary(env)
        self.assertEqual(record._load_state(), (SALT, {"d1": ("lbl", "secret")}))
        self.assertIsInstance(record._load_state()[1]["d1"], tuple)
        self.assertEqual(env.kv.reads, 1, "second call served from the cache")
        self.clock.now += 29.9
        record._load_state()
        self.assertEqual(env.kv.reads, 1)
        self.clock.now += 0.1  # exactly 30 s after the load
        record._load_state()
        self.assertEqual(env.kv.reads, 2, "the cache expires after CACHE_SECONDS")
        self.assertEqual(secret_canary.CACHE_SECONDS, 30)

    def test_the_cache_is_per_database(self):
        raw = '{"salt": "s1", "table": {}}'
        first, second = _ModelEnv(kv=_Kv(raw), dbname="a"), _ModelEnv(kv=_Kv('{"salt": "s2", "table": {}}'), dbname="b")
        self.assertEqual(_canary(first)._load_state()[0], "s1")
        self.assertEqual(_canary(second)._load_state()[0], "s2")
        self.assertEqual(second.kv.reads, 1)

    def test_store_state_writes_json_and_drops_the_cached_copy(self):
        env = _ModelEnv()
        record = _canary(env)
        record._load_state()
        self.assertIn("db1", secret_canary._tables)
        secret_canary._tables["other"] = (0, "s", {})
        record._store_state("salty", {"d": ("l", "secret")})
        self.assertEqual(env.kv.writes, ['{"salt": "salty", "table": {"d": ["l", "secret"]}}'])
        self.assertNotIn("db1", secret_canary._tables)
        self.assertIn("other", secret_canary._tables, "only this database's entry is dropped")

    def test_register_secret_makes_a_salt_once_and_stores_only_the_fingerprint(self):
        env = _ModelEnv()
        record = _canary(env)
        record._register_secret("first", TOKEN)
        salt, table = record._load_state()
        self.assertRegex(salt, r"^[0-9a-f]{32}$")
        self.assertEqual(table, {detector.fingerprint(salt, TOKEN): ("first", "secret")})
        self.assertNotIn(TOKEN, env.kv.raw)
        record._register_secret("second", OTHER, detector.KIND_CANARY)
        salt_again, table = record._load_state()
        self.assertEqual(salt_again, salt, "the salt is made once")
        self.assertEqual(table, {detector.fingerprint(salt, TOKEN): ("first", "secret"),
                                 detector.fingerprint(salt, OTHER): ("second", "canary")})

    def test_register_secret_refuses_a_value_that_is_not_one_token(self):
        env = _ModelEnv()
        for junk in ("short", "has space in it 1234567", "", None, "a" * 513):
            with self.assertRaises(ValueError) as caught:
                _canary(env)._register_secret("lbl", junk)
            self.assertEqual(str(caught.exception),
                             "lbl is not a single 16-512 character token of letters, digits and _-+/.")
        self.assertEqual(env.kv.writes, [])

    def test_register_secret_does_not_change_the_cached_table_in_place(self):
        env = _ModelEnv(kv=_Kv('{"salt": "%s", "table": {}}' % SALT))
        record = _canary(env)
        _salt, before = record._load_state()
        record._register_secret("x", TOKEN)
        self.assertEqual(before, {}, "the table object handed out earlier is untouched")

    def test_plant_canaries_creates_one_decoy_per_shape_registers_it_and_is_idempotent(self):
        data = tempfile.mkdtemp(prefix="hsc-")
        self.addCleanup(shutil.rmtree, data, True)
        self.safe_patch_object(secret_canary, "config", new={"data_dir": data})
        self.safe_patch_object(os.environ, "setdefault", new=lambda *a: None)
        env = _ModelEnv()
        record = _canary(env)
        record._plant_canaries()
        self.assertEqual([key for key, _value in env.params.sets],
                         ["stripe_secret_key_old", "s3_secret_access_key_old", "cloudflare_api_token_old"])
        salt, table = record._load_state()
        self.assertEqual(sorted(label for label, _kind in table.values()), sorted(detector.CANARY_SHAPES))
        self.assertEqual({kind for _label, kind in table.values()}, {"canary"})
        for key, value in env.params.sets:
            self.assertIn(detector.fingerprint(salt, value), table)
        keys = os.path.join(data, "keys")
        self.assertEqual(sorted(os.listdir(keys)), sorted("%s_old" % label for label in detector.CANARY_SHAPES))
        shutil.rmtree(keys)
        env.params.sets.clear()
        record._plant_canaries()
        self.assertEqual(env.params.sets, [], "nothing planted twice")
        self.assertEqual(len(os.listdir(keys)), 3, "the decoy files are put back even when nothing new is planted")

    def test_plant_canaries_leaves_an_existing_parameter_alone_and_unregistered(self):
        data = tempfile.mkdtemp(prefix="hsc-")
        self.addCleanup(shutil.rmtree, data, True)
        self.safe_patch_object(secret_canary, "config", new={"data_dir": data})
        self.safe_patch_object(os.environ, "setdefault", new=lambda *a: None)
        env = _ModelEnv(params=_Params({"stripe_secret_key_old": "an-operator-value"}))
        record = _canary(env)
        record._plant_canaries()
        self.assertEqual(env.params.values["stripe_secret_key_old"], "an-operator-value")
        _salt, table = record._load_state()
        self.assertEqual(sorted(label for label, _kind in table.values()), ["cloudflare_api_token", "s3_secret_access_key"])

    def test_plant_canaries_skips_labels_already_registered_as_canaries_but_not_as_secrets(self):
        data = tempfile.mkdtemp(prefix="hsc-")
        self.addCleanup(shutil.rmtree, data, True)
        self.safe_patch_object(secret_canary, "config", new={"data_dir": data})
        self.safe_patch_object(os.environ, "setdefault", new=lambda *a: None)
        env = _ModelEnv()
        record = _canary(env)
        record._register_secret("stripe_secret_key", TOKEN, "canary")
        record._register_secret("s3_secret_access_key", OTHER, "secret")
        record._plant_canaries()
        planted = [key for key, _value in env.params.sets]
        self.assertEqual(planted, ["s3_secret_access_key_old", "cloudflare_api_token_old"])

    def _decoy_env(self, params):
        data = tempfile.mkdtemp(prefix="hsc-")
        self.addCleanup(shutil.rmtree, data, True)
        self.safe_patch_object(secret_canary, "config", new={"data_dir": data})
        return data, _canary(_ModelEnv(params=_Params(params)))

    def test_decoy_file_and_variable_are_written_with_private_modes(self):
        values = {"stripe_secret_key_old": "decoy_" + "A" * 24, "s3_secret_access_key_old": "S" * 40}
        data, record = self._decoy_env(values)
        self._isolate_decoy_variables()
        record._plant_decoy_file_and_env()
        keys = os.path.join(data, "keys")
        self.assertEqual(stat.S_IMODE(os.stat(keys).st_mode), 0o700)
        self.assertEqual(sorted(os.listdir(keys)), ["s3_secret_access_key_old", "stripe_secret_key_old"],
                         "no file for a label with no parameter")
        path = os.path.join(keys, "stripe_secret_key_old")
        self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o600)
        with open(path) as handle:
            self.assertEqual(handle.read(), "decoy_" + "A" * 24 + "\n")
        self.assertEqual(os.environ["STRIPE_SECRET_KEY_OLD"], "decoy_" + "A" * 24)
        self.assertEqual(os.environ["S3_SECRET_ACCESS_KEY_OLD"], "S" * 40)
        self.assertNotIn("CLOUDFLARE_API_TOKEN_OLD", os.environ)

    def test_an_existing_file_or_variable_is_not_replaced(self):
        data, record = self._decoy_env({"stripe_secret_key_old": "NEWVALUE" + "x" * 20})
        keys = os.path.join(data, "keys")
        os.makedirs(keys)
        path = os.path.join(keys, "stripe_secret_key_old")
        with open(path, "w") as handle:
            handle.write("kept\n")
        self._isolate_decoy_variables()
        os.environ["STRIPE_SECRET_KEY_OLD"] = "from-the-operator"
        with self.assertNoLogs(secret_canary._logger, "WARNING"):
            record._plant_decoy_file_and_env()
        with open(path) as handle:
            self.assertEqual(handle.read(), "kept\n")
        self.assertEqual(os.environ["STRIPE_SECRET_KEY_OLD"], "from-the-operator")

    def _isolate_decoy_variables(self):
        """The real install may already have exported the decoys into this process: set them aside for the test."""
        names = ("STRIPE_SECRET_KEY_OLD", "S3_SECRET_ACCESS_KEY_OLD", "CLOUDFLARE_API_TOKEN_OLD")
        saved = {name: os.environ.pop(name, None) for name in names}

        def restore():
            for name in names:
                os.environ.pop(name, None)
            os.environ.update({name: value for name, value in saved.items() if value is not None})
        self.addCleanup(restore)

    def test_a_directory_that_cannot_be_written_is_logged_and_skipped(self):
        data, record = self._decoy_env({"stripe_secret_key_old": "V" * 30, "s3_secret_access_key_old": "W" * 40})
        with open(os.path.join(data, "keys"), "w") as handle:  # a file where the directory should be
            handle.write("x")
        self._isolate_decoy_variables()
        with self.assertLogs(secret_canary._logger, "WARNING") as logs:
            record._plant_decoy_file_and_env()
        self.assertEqual([r.getMessage() for r in logs.records], [
            "Decoy key file stripe_secret_key not written: FileExistsError",
            "Decoy key file s3_secret_access_key not written: FileExistsError"])
        self.assertEqual(os.environ["S3_SECRET_ACCESS_KEY_OLD"], "W" * 40, "the loop goes on after a failure")

    def test_scan_text_raises_an_alarm_per_match_with_the_place_redacted(self):
        raw = '{"salt": "%s", "table": {"%s": ["lbl", "secret"], "%s": ["two", "canary"]}}' % (
            SALT, _digest(TOKEN), _digest(OTHER))
        env = _ModelEnv(kv=_Kv(raw))
        record = _canary(env)
        found = record._scan_text("leak %s and %s" % (TOKEN, OTHER), "response", "/p/%s" % TOKEN)
        self.assertEqual(found, [("lbl", "secret"), ("two", "canary")])
        self.assertEqual(self.alarms, [(env, alerts.Alarm("lbl", "secret", "response", "/p/[REDACTED:lbl]")),
                                       (env, alerts.Alarm("two", "canary", "response", "/p/[REDACTED:lbl]"))])
        del self.alarms[:]
        self.assertEqual(record._scan_text("clean text with nothing", "log", "x"), [])
        self.assertEqual(self.alarms, [])

    def test_scan_text_with_no_table_does_nothing(self):
        record = _canary(_ModelEnv())
        self.assertEqual(record._scan_text("anything at all goes here " + TOKEN, "response", "/"), [])
        self.assertEqual(self.alarms, [])

    def test_register_hook_loads_state_plants_and_installs_each_hook_once(self):
        order = []
        record = _canary(_ModelEnv())
        self.safe_patch_object(secret_canary.SecretCanary, "_load_state", new=lambda self: order.append("load"))
        self.safe_patch_object(secret_canary.SecretCanary, "_plant_decoy_file_and_env",
                               new=lambda self: order.append("plant"))
        factory_calls, saved_factory = [], logging.getLogRecordFactory()
        self.addCleanup(logging.setLogRecordFactory, saved_factory)
        original_call = secret_canary.http.Application.__call__
        self.addCleanup(setattr, secret_canary.http.Application, "__call__", original_call)
        saved_originals = (list(secret_canary._original_factory), list(secret_canary._original_call))
        del secret_canary._original_factory[:], secret_canary._original_call[:]
        self.addCleanup(lambda: (secret_canary._original_factory.__setitem__(slice(None), saved_originals[0]),
                                 secret_canary._original_call.__setitem__(slice(None), saved_originals[1])))
        self.safe_patch_object(secret_canary, "_watch_log_records", new=lambda factory: factory_calls.append(factory) or factory)
        self.safe_patch_object(models.BaseModel, "_register_hook", new=lambda self: order.append("super"))
        wrapper = object()
        wraps = []
        self.safe_patch_object(secret_canary.wsgi_scan, "scan_wsgi",
                               new=lambda app, scan: wraps.append((app, scan)) or wrapper)
        secret_canary.SecretCanary._register_hook(record)
        self.assertEqual(order, ["super", "load", "plant"])
        self.assertEqual(wraps, [(original_call, secret_canary.scan_after_request)])
        self.assertIs(secret_canary.http.Application.__call__, wrapper)
        self.assertEqual(factory_calls, [saved_factory])
        self.assertEqual(secret_canary._original_factory, [saved_factory])
        self.assertEqual(secret_canary._original_call, [original_call])
        wrapped = secret_canary.http.Application.__call__
        secret_canary.SecretCanary._register_hook(record)
        self.assertEqual(factory_calls, [saved_factory], "installed once")
        self.assertIs(secret_canary.http.Application.__call__, wrapped)
        self.assertEqual(secret_canary._original_call, [original_call])


@tagged("post_install", "-at_install")
class TestLogWatcherUnits(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:watch_log_records]

    def setUp(self):
        super().setUp()
        saved = dict(secret_canary._tables)
        secret_canary._tables.clear()
        self.addCleanup(lambda: (secret_canary._tables.clear(), secret_canary._tables.update(saved)))
        self.alarms = []
        self.safe_patch_object(alerts, "logging_backend", new=lambda env, alarm: self.alarms.append((env, alarm)))

    def _make(self, name, msg, *args):
        def factory(name, level, fn, lno, msg, args, exc_info, func=None, sinfo=None, **kwargs):
            return logging.LogRecord(name, level, fn, lno, msg, args, exc_info, func, sinfo)
        return secret_canary._watch_log_records(factory)(name, logging.INFO, "f.py", 1, msg, args, None)

    def _loaded(self, **labels):
        secret_canary._tables["db1"] = (0, SALT, _table(**labels))

    def test_a_record_with_a_registered_secret_is_redacted_and_alarmed(self):
        self._loaded(lbl=TOKEN, two=OTHER)
        record = self._make("some.logger", "key=%s other=%s", TOKEN, OTHER)
        self.assertEqual(record.getMessage(), "key=[REDACTED:lbl] other=[REDACTED:two]")
        self.assertIsNone(record.args)
        self.assertEqual(self.alarms, [(None, alerts.Alarm("lbl", "secret", "log", "some.logger")),
                                       (None, alerts.Alarm("two", "secret", "log", "some.logger"))])

    def test_a_clean_record_is_left_as_it_was_made(self):
        self._loaded(lbl=TOKEN)
        record = self._make("some.logger", "hello %s", "world")
        self.assertEqual(record.msg, "hello %s")
        self.assertEqual(record.args, ("world",))
        self.assertEqual(self.alarms, [])

    def test_with_no_loaded_state_or_an_empty_table_nothing_is_scanned(self):
        with self.assertNoLogs(secret_canary._logger, "WARNING"):
            record = self._make("some.logger", "%s %s", "malformed: a scan would fail and say so")
        self.assertEqual(record.args, ("malformed: a scan would fail and say so",))
        record = self._make("some.logger", "key=%s", TOKEN)
        self.assertEqual(record.getMessage(), "key=" + TOKEN)
        secret_canary._tables["db1"] = (0, SALT, {})
        record = self._make("some.logger", "key=%s", TOKEN)
        self.assertEqual(record.getMessage(), "key=" + TOKEN)
        self.assertEqual(self.alarms, [])

    def test_the_canarys_own_loggers_are_not_scanned(self):
        self._loaded(lbl=TOKEN)
        record = self._make("hams_secret_canary.alarm", "key=%s", TOKEN)
        self.assertEqual(record.getMessage(), "key=" + TOKEN)
        self.assertEqual(self.alarms, [])
        record = self._make("hams_secret_canary", "key=%s", TOKEN)
        self.assertEqual(record.getMessage(), "key=" + TOKEN)
        record = self._make("xhams_secret_canary.alarm", "key=%s", TOKEN)
        self.assertEqual(record.getMessage(), "key=[REDACTED:lbl]", "only a name that starts with the prefix is skipped")

    def test_the_first_database_with_a_hit_wins_and_a_database_without_a_table_is_skipped(self):
        secret_canary._tables["empty"] = (0, "s0", {})
        secret_canary._tables["one"] = (0, SALT, _table(lbl=TOKEN))
        secret_canary._tables["two"] = (0, SALT, _table(other=TOKEN))
        record = self._make("some.logger", "key=%s", TOKEN)
        self.assertEqual(record.getMessage(), "key=[REDACTED:lbl]")
        self.assertEqual(len(self.alarms), 1, "the scan stops at the first database that matched")

    def test_a_malformed_call_never_breaks_the_caller_and_is_reported_by_type_only(self):
        self._loaded(lbl=TOKEN)
        with self.assertLogs(secret_canary._logger, "WARNING") as logs:
            record = self._make("some.logger", "%s %s", "only one argument")
        self.assertEqual(logs.records[0].getMessage(), "Secret canary: scanning a log record failed: TypeError")
        self.assertEqual(record.msg, "%s %s", "the record is left as made")
        self.assertEqual(self.alarms, [])


class _FakeRegistry:
    cursors = []
    created = []

    def __init__(self, dbname):
        self.dbname = dbname
        _FakeRegistry.created.append(dbname)

    @contextmanager
    def cursor(self):
        cursor = _FakeCursor()
        _FakeRegistry.cursors.append(cursor)
        yield cursor


class _FakeCursor:
    def __init__(self):
        self.statements = []
        self.saved = 0

    def savepoint(self):
        cursor = self

        @contextmanager
        def manager():
            cursor.saved += 1
            yield
        return manager()

    def execute(self, query, params=()):
        self.statements.append((query, params))

    def fetchone(self):
        return (4242,)


@tagged("post_install", "-at_install")
class TestScanAfterRequestUnits(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:scan_after_request]
    # Tests [@ANCHOR: hams_secret_canary:post_dispatch_scan]

    def setUp(self):
        super().setUp()
        saved = dict(secret_canary._tables)
        secret_canary._tables.clear()
        self.addCleanup(lambda: (secret_canary._tables.clear(), secret_canary._tables.update(saved)))
        del _FakeRegistry.cursors[:], _FakeRegistry.created[:]
        self.safe_patch_object(secret_canary, "Registry", new=_FakeRegistry)
        self.envs = []
        self.safe_patch_object(secret_canary, "api", new=SimpleNamespace(
            Environment=lambda cr, uid, context: self.envs.append((cr, uid, context)) or ("ENV", uid)))
        self.alarms = []
        self.safe_patch_object(alerts, "raise_alarm", new=lambda env, alarm: self.alarms.append((env, alarm)))

    def test_a_hit_raises_the_alarms_on_a_fresh_cursor_as_the_facility_account(self):
        secret_canary._tables["db1"] = (0, SALT, _table(lbl=TOKEN, two=OTHER))
        secret_canary.scan_after_request("leak %s %s" % (TOKEN, OTHER), "/x/%s" % TOKEN)
        self.assertEqual(_FakeRegistry.created, ["db1"])
        cursor = _FakeRegistry.cursors[0]
        self.assertEqual(cursor.statements, [("SELECT zero_sudo_get_service_uid(%s)", ("zero_sudo.odoo_facility_service_internal",))])
        self.assertEqual(cursor.saved, 1)
        self.assertEqual(self.envs, [(cursor, 4242, {})])
        self.assertEqual(self.alarms, [
            (("ENV", 4242), alerts.Alarm("lbl", "secret", "response", "/x/[REDACTED:lbl]")),
            (("ENV", 4242), alerts.Alarm("two", "secret", "response", "/x/[REDACTED:lbl]"))])

    def test_no_hit_opens_no_cursor_and_each_database_is_scanned_with_its_own_table(self):
        secret_canary._tables["db1"] = (0, SALT, _table(lbl=TOKEN))
        secret_canary._tables["db2"] = (0, "salt-two-0123456789abcdef", _table(lbl=TOKEN))
        secret_canary.scan_after_request("a clean body of text", "/x")
        self.assertEqual(_FakeRegistry.created, [])
        secret_canary.scan_after_request("leak " + TOKEN, "/x")
        self.assertEqual(_FakeRegistry.created, ["db1"], "db2 has another salt, so the same text does not match there")
        secret_canary.scan_after_request("leak " + TOKEN, "/x")

    def test_a_failure_is_logged_and_swallowed(self):
        secret_canary._tables["db1"] = (0, SALT, _table(lbl=TOKEN))

        def boom(env, alarm):
            raise RuntimeError("alert backend down")
        self.safe_patch_object(alerts, "raise_alarm", new=boom)
        with self.assertLogs(secret_canary._logger, "ERROR") as logs:
            self.assertIsNone(secret_canary.scan_after_request("leak " + TOKEN, "/x"))
        self.assertEqual(logs.records[0].getMessage(), "Secret canary: scanning a response failed")
        self.assertIsNotNone(logs.records[0].exc_info)

    def _post_dispatch(self, response, request):
        calls = []
        self.safe_patch_object(models.BaseModel, "_post_dispatch", new=classmethod(lambda cls, r: ("RESULT", r)), create=True)
        self.safe_patch_object(ir_http, "request", new=request)
        self.safe_patch_object(wsgi_scan, "mark_scanned", new=lambda: calls.append("marked"))
        return ir_http.IrHttp._post_dispatch(response), calls

    def _response(self, mimetype="text/html", body=b"hello"):
        return SimpleNamespace(mimetype=mimetype, direct_passthrough=False, is_sequence=True,
                               get_data=lambda: body)

    def test_post_dispatch_scans_a_text_response_without_changing_it(self):
        scanned = []
        env = {"hams.secret.canary": SimpleNamespace(_scan_text=lambda *a: scanned.append(a))}
        request = SimpleNamespace(env=env, httprequest=SimpleNamespace(path="/the/path"))
        response = self._response(body=b"abc" + b"\xff")
        result, calls = self._post_dispatch(response, request)
        self.assertEqual(result, ("RESULT", response))
        self.assertEqual(calls, ["marked"])
        self.assertEqual(scanned, [("abc�", "response", "/the/path")])

    def test_post_dispatch_scans_only_the_first_bytes(self):
        scanned = []
        env = {"hams.secret.canary": SimpleNamespace(_scan_text=lambda *a: scanned.append(a))}
        request = SimpleNamespace(env=env, httprequest=SimpleNamespace(path="/p"))
        self.safe_patch_object(detector, "MAX_SCAN_BYTES", new=4)
        self._post_dispatch(self._response(body=b"abcdefgh"), request)
        self.assertEqual(scanned, [("abcd", "response", "/p")])

    def test_post_dispatch_skips_binary_responses_and_outside_a_request(self):
        scanned = []
        env = {"hams.secret.canary": SimpleNamespace(_scan_text=lambda *a: scanned.append(a))}
        request = SimpleNamespace(env=env, httprequest=SimpleNamespace(path="/p"))
        result, calls = self._post_dispatch(self._response("image/png"), request)
        self.assertEqual((calls, scanned), ([], []))
        self.assertEqual(result[0], "RESULT")
        result, calls = self._post_dispatch(self._response(), None)
        self.assertEqual((calls, scanned), ([], []))
        self.assertEqual(result[0], "RESULT")

    def test_post_dispatch_never_lets_a_scan_failure_reach_the_response(self):
        def boom(*a):
            raise RuntimeError("closed cursor")
        env = {"hams.secret.canary": SimpleNamespace(_scan_text=boom)}
        request = SimpleNamespace(env=env, httprequest=SimpleNamespace(path="/p"))
        with self.assertLogs(ir_http._logger, "ERROR") as logs:
            result, calls = self._post_dispatch(self._response(), request)
        self.assertEqual(result[0], "RESULT")
        self.assertEqual(calls, ["marked"])
        self.assertEqual(logs.records[0].getMessage(), "Secret canary: scanning a response failed")


@tagged("post_install", "-at_install")
class TestPostInitHook(HamsTransactionCase):
    # Tests [@ANCHOR: hams_secret_canary:post_init_hook]

    def test_the_hook_plants_the_canaries_of_this_install(self):
        calls = []
        env = {"hams.secret.canary": SimpleNamespace(_plant_canaries=lambda: calls.append("planted"))}
        self.assertIsNone(hooks.post_init_hook(env))
        self.assertEqual(calls, ["planted"])
