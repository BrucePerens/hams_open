# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging
import os
import stat
import tempfile
import sys
import time
import xmlrpc.client
from unittest.mock import patch

from odoo.tests import tagged
from odoo.tests.common import release_test_lock
from odoo.tools import config
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_secret_canary import alerts, detector, wsgi_scan
from odoo.addons.hams_secret_canary.models import secret_canary

SECRET = "whsec_Zk39dQ7mPq2LxV8nTb4RcY6a"
PAGE = "<html><body><a href='/web/login'>Log in</a> " + "<p>ordinary text with words and numbers 12345</p>" * 50 + "</body></html>"


class CanaryCase(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.canary = self.env["hams.secret.canary"]
        secret_canary._tables.clear()
        self.addCleanup(secret_canary._tables.clear)
        self.env.ref("base.user_admin").partner_id.email = "admin@example.test"  # the mail backend's recipient
        self.canary._register_secret("test_secret", SECRET)
        self.salt, self.table = self.canary._load_state()
        self.addCleanup(self._restore_backends, dict(alerts.BACKENDS))

    def _restore_backends(self, saved):
        alerts.BACKENDS.clear()
        alerts.BACKENDS.update(saved)

    def _log_rows(self):
        return self.env["zero_sudo.security.log"].search([("reason", "=", "secret_canary")])


@tagged("post_install", "-at_install")
class TestDetector(CanaryCase):
    """The scan: what is found, what is not, and what it costs."""
    # Tests [@ANCHOR: hams_secret_canary:fingerprint]
    # Tests [@ANCHOR: hams_secret_canary:new_salt]
    # Tests [@ANCHOR: hams_secret_canary:make_canary]
    # Tests [@ANCHOR: hams_secret_canary:is_token]
    # Tests [@ANCHOR: hams_secret_canary:find_matches]
    # Tests [@ANCHOR: hams_secret_canary:candidates]
    # Tests [@ANCHOR: hams_secret_canary:redact]

    def test_secret_in_json_html_and_url_is_found_once_each(self):
        for text in ('{"key": "%s"}' % SECRET, "<input value='%s'>" % SECRET, "/x?k=%s&y=1" % SECRET,
                     "token %s." % SECRET):
            self.assertEqual(detector.find_matches(text, self.salt, self.table), [("test_secret", "secret")], text)

    def test_ordinary_responses_have_no_false_positives(self):
        self.assertEqual(detector.find_matches(PAGE, self.salt, self.table), [])
        self.assertEqual(detector.find_matches("sk_live_" + "A" * 24, self.salt, self.table), [])  # right shape, not registered
        self.assertEqual(detector.find_matches(SECRET[:-1], self.salt, self.table), [])

    def test_only_the_hash_is_stored(self):
        self.assertNotIn(SECRET, repr(self.table))
        self.assertIn(detector.fingerprint(self.salt, SECRET), self.table)

    def test_redact_removes_the_value(self):
        redacted = detector.redact('{"k": "%s"} ok' % SECRET, self.salt, self.table)
        self.assertEqual(redacted, '{"k": "[REDACTED:test_secret]"} ok')

    def test_one_megabyte_scans_fast(self):
        body = "".join("<p>x%d long_identifier_number_%d_abcdefghij</p>" % (i, i) for i in range(22000))
        self.assertGreaterEqual(len(body), 900000)
        start = time.monotonic()
        self.assertEqual(detector.find_matches(body, self.salt, self.table), [])
        self.assertLess(time.monotonic() - start, 1.0)

    def test_canaries_are_random_per_call_and_shaped(self):
        self.assertNotEqual(detector.make_canary("stripe_secret_key"), detector.make_canary("stripe_secret_key"))
        self.assertTrue(detector.make_canary("stripe_secret_key").startswith("sk_live_"))
        for label in detector.CANARY_SHAPES:
            self.assertTrue(detector.is_token(detector.make_canary(label)))
        self.assertNotEqual(detector.new_salt(), detector.new_salt())

    def test_registering_a_non_token_fails(self):
        for bad in ("", "short", "has space in the middle of it", "trailing.dot.token.value."):
            with self.assertRaises(ValueError):
                self.canary._register_secret("bad", bad)


@tagged("post_install", "-at_install")
class TestAlarm(CanaryCase):
    """A hit raises every backend's alarm, and no alarm carries the matched value."""
    # Tests [@ANCHOR: hams_secret_canary:register_secret]
    # Tests [@ANCHOR: hams_secret_canary:load_state]
    # Tests [@ANCHOR: hams_secret_canary:store_state]
    # Tests [@ANCHOR: hams_secret_canary:scan_text]
    # Tests [@ANCHOR: hams_secret_canary:raise_alarm]
    # Tests [@ANCHOR: hams_secret_canary:register_backend]
    # Tests [@ANCHOR: hams_secret_canary:logging_backend]
    # Tests [@ANCHOR: hams_secret_canary:record_backend]
    # Tests [@ANCHOR: hams_secret_canary:mail_backend]

    def test_secret_in_a_response_body_raises_the_alarm_without_the_value(self):
        mails_before = self.env["mail.mail"].search([]).ids
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
            found = self.canary._scan_text('{"key": "%s"}' % SECRET, "response", "/leak/%s" % SECRET)
        self.assertEqual(found, [("test_secret", "secret")])
        rows = self._log_rows()
        self.assertEqual(rows.mapped("login"), ["test_secret"])
        mails = self.env["mail.mail"].search([("id", "not in", mails_before), ("subject", "like", "Secret canary")])
        self.assertEqual(mails.mapped("email_to"), ["admin@example.test"])
        for text in (*logs.output, rows.user_agent, mails.subject, mails.body_html, mails.email_to):
            self.assertNotIn(SECRET, text)
        self.assertIn("[REDACTED:test_secret]", rows.user_agent)

    def test_a_clean_response_raises_nothing(self):
        with self.assertNoLogs("hams_secret_canary.alarm"):
            self.assertEqual(self.canary._scan_text(PAGE, "response", "/odoo"), [])
        self.assertFalse(self._log_rows())

    def test_a_planted_canary_raises_the_alarm_as_a_canary(self):
        self.canary._plant_canaries()
        params = self.env["ir.config_parameter"]  # the admin reads it as an attacker would; service accounts may not
        value = params.get_param("stripe_secret_key_old")
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
            self.assertEqual(self.canary._scan_text("leaked " + value, "response", "/x"), [("stripe_secret_key", "canary")])
        self.assertIn("canary stripe_secret_key", logs.output[0])

    def test_backend_interface_takes_a_second_backend(self):
        seen = []
        alerts.register_backend("collector")(lambda env, alarm: seen.append(alarm))
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL"):
            self.canary._scan_text(SECRET, "response", "/x")
        self.assertEqual(seen, [alerts.Alarm("test_secret", "secret", "response", "/x")])

    def test_a_log_line_with_a_registered_secret_raises_the_alarm_and_is_redacted(self):
        # Tests [@ANCHOR: hams_secret_canary:register_hook]
        # Tests [@ANCHOR: hams_secret_canary:watch_log_records]
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as alarms, self.assertLogs("some.addon", "WARNING") as lines:
            logging.getLogger("some.addon").warning("login failed with key %s for user", SECRET)
        self.assertEqual(lines.output, ["WARNING:some.addon:login failed with key [REDACTED:test_secret] for user"])
        for text in alarms.output:
            self.assertNotIn(SECRET, text)
        self.assertIn("log", alarms.output[0])


@tagged("post_install", "-at_install")
class TestPlanting(CanaryCase):
    """The decoys are created per install, once."""
    # Tests [@ANCHOR: hams_secret_canary:plant_canaries]
    # Tests [@ANCHOR: hams_secret_canary:post_init_hook]
    # Tests [@ANCHOR: hams_secret_canary:plant_decoy_file_and_env]

    def test_decoys_are_also_an_environment_variable_and_a_private_file(self):
        self.canary._plant_canaries()
        params = self.env["ir.config_parameter"]
        value = params.get_param("stripe_secret_key_old")
        with tempfile.TemporaryDirectory() as data_dir, \
                patch.dict(config.options, {"data_dir": data_dir}), patch.dict(os.environ):
            os.environ.pop("STRIPE_SECRET_KEY_OLD", None)
            self.canary._plant_decoy_file_and_env()
            path = os.path.join(data_dir, "keys", "stripe_secret_key_old")
            with open(path) as handle:
                self.assertEqual(handle.read().strip(), value)
            self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o600)
            self.assertEqual(os.environ["STRIPE_SECRET_KEY_OLD"], value)
            self.canary._plant_decoy_file_and_env()  # a second call leaves the existing file and variable alone
            with open(path) as handle:
                self.assertEqual(handle.read().strip(), value)

    def test_planting_is_idempotent_and_registers_each_decoy(self):
        params = self.env["ir.config_parameter"]  # the admin reads it as an attacker would; service accounts may not
        self.canary._plant_canaries()
        first = {label: params.get_param("%s_old" % label) for label in detector.CANARY_SHAPES}
        self.assertTrue(all(first.values()))
        self.canary._plant_canaries()
        self.assertEqual(first, {label: params.get_param("%s_old" % label) for label in detector.CANARY_SHAPES})
        salt, table = self.canary._load_state()
        self.assertEqual({label for label, kind in table.values() if kind == "canary"}, set(detector.CANARY_SHAPES))
        self.assertNotIn(first["stripe_secret_key"], repr(table))


@tagged("post_install", "-at_install")
class TestResponseHook(CanaryCase, HamsHttpCase):
    """The `ir.http` hook, through a real request."""
    # Tests [@ANCHOR: hams_secret_canary:post_dispatch_scan]
    # Tests [@ANCHOR: hams_secret_canary:scannable_response]

    def test_a_response_carrying_a_registered_token_raises_the_alarm(self):
        page = self.url_open("/odoo").text
        token = next(t for t in sorted({m.group() for m in detector.TOKEN_RE.finditer(page)}, key=len, reverse=True)
                     if "static" in t)
        self.canary._register_secret("page_token", token)
        secret_canary._tables.clear()
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
            self.url_open("/odoo")
        self.assertIn("page_token", logs.output[0])


def _wsgi(body, content_type="text/xml", chunks=1):
    """A WSGI application answering `body` in `chunks` pieces."""
    def application(environ, start_response):
        start_response("200 OK", [("Content-Type", content_type)])
        size = max(1, len(body) // chunks)
        return [body[i:i + size] for i in range(0, len(body), size)]
    return application


@tagged("post_install", "-at_install")
class TestWsgiScan(HamsTransactionCase):
    """The WSGI wrapper: what it scans and that it never changes the response."""
    # Tests [@ANCHOR: hams_secret_canary:scan_wsgi]

    def _run(self, application, environ=None):
        seen = []
        wrapped = wsgi_scan.scan_wsgi(application, lambda text, path: seen.append((text, path)))
        sent = []
        body = b"".join(wrapped(dict(environ or {}, PATH_INFO="/xmlrpc/2/common"), lambda status, headers, exc_info=None: sent.append(status)))
        return body, seen, sent

    # Tests [@ANCHOR: hams_secret_canary:is_text]
    # Tests [@ANCHOR: hams_secret_canary:pass_through]
    # Tests [@ANCHOR: hams_secret_canary:scan_chunks]
    def test_text_is_scanned_after_it_is_sent_unchanged_even_when_split_in_chunks(self):
        payload = b"<fault>" + SECRET.encode() + b"</fault>"
        body, seen, sent = self._run(_wsgi(payload, chunks=4))
        self.assertEqual((body, sent), (payload, ["200 OK"]))
        self.assertEqual(seen, [(payload.decode(), "/xmlrpc/2/common")])

    def test_a_response_post_dispatch_already_scanned_is_not_scanned_again(self):
        self.assertEqual(self._run(_wsgi(b"x"), {wsgi_scan.SCANNED: True})[1], [])

    # Tests [@ANCHOR: hams_secret_canary:is_text]
    def test_binary_responses_are_not_scanned(self):
        self.assertEqual(self._run(_wsgi(b"x", "image/png"))[1], [])

    # Tests [@ANCHOR: hams_secret_canary:scan_chunks]
    def test_only_the_first_megabyte_is_scanned(self):
        body, seen, _sent = self._run(_wsgi(b"a" * (detector.MAX_SCAN_BYTES + 5000), chunks=3))
        self.assertEqual(len(body), detector.MAX_SCAN_BYTES + 5000)
        self.assertEqual(len(seen[0][0]), detector.MAX_SCAN_BYTES)


@tagged("post_install", "-at_install")
class TestWsgiHook(CanaryCase, HamsHttpCase):
    """A real XML-RPC fault, which a controller builds itself and `_post_dispatch` never sees."""
    # Tests [@ANCHOR: hams_secret_canary:scan_after_request]

    def test_a_fault_naming_a_registered_secret_raises_the_alarm_once_without_the_value(self):
        self.assertTrue(secret_canary._tables)  # loaded by setUp, as the process has it after start-up
        request_xml = "<?xml version='1.0'?><methodCall><methodName>%s</methodName><params></params></methodCall>" % SECRET
        # hams_base, when installed with this module (the gate does), scrubs XML-RPC faults so they no longer echo the
        # method name; this test needs the stock fault that does, so put a verbatim one in for the request.
        rpc = sys.modules["odoo.addons.rpc.controllers.xmlrpc"]
        self.safe_patch_object(rpc, "xmlrpc_handle_exception_int", lambda error: rpc.dumps(xmlrpc.client.Fault(1, str(error))))
        with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
            response = self.url_open("/xmlrpc/2/common", data=request_xml, headers={"Content-Type": "text/xml"})
            # The scan runs on the server thread once the last chunk is out and opens its own cursor, which in test
            # mode needs the registry test lock the test thread holds again as soon as the response is in. Give the
            # lock up while waiting; leaving the block takes it back, which also waits until the scan has finished.
            with release_test_lock():
                deadline = time.monotonic() + 10
                while not logs.records and time.monotonic() < deadline:
                    time.sleep(0.05)
        self.assertIn(SECRET, response.text)  # the fault does echo it: the leak this scan exists for
        self.assertEqual(len(logs.output), 1)
        self.assertIn("test_secret", logs.output[0])
        self.assertNotIn(SECRET, logs.output[0])
