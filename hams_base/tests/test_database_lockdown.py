# -*- coding: utf-8 -*-
import json
from types import SimpleNamespace

from werkzeug.exceptions import BadRequest
from unittest.mock import patch

from odoo import http, tools
from odoo.exceptions import AccessDenied, UserError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_base.controllers import database_lockdown as lockdown


def _dev_mode(value):
    return patch.dict(tools.config.options, {"dev_mode": value})


@tagged("post_install", "-at_install")
class TestScrubErrorBody(HamsTransactionCase):
    """Tests [@ANCHOR: hams_base:scrub_error_body] and [@ANCHOR: hams_base:developer_mode_check]"""

    def _raised(self, exc_type, *args):
        """An exception instance that was really raised (so it carries a traceback)."""
        with self.assertRaises(exc_type) as caught:
            raise exc_type(*args)
        return caught.exception

    def _body(self, exc):
        return http.serialize_exception(exc)

    def test_traceback_is_dropped_and_unexpected_errors_become_generic(self):
        exc = self._raised(KeyError, "secret_table_name")
        raw = self._body(exc)
        self.assertIn("secret_table_name", raw["debug"])
        with _dev_mode([]):
            scrubbed = lockdown.scrub_error_body(raw, exc)
        self.assertEqual(scrubbed["debug"], "")
        self.assertEqual(scrubbed["message"], lockdown.GENERIC_MESSAGE)
        self.assertNotIn("secret_table_name", json.dumps(scrubbed))
        self.assertEqual(scrubbed["arguments"], [lockdown.GENERIC_MESSAGE])

    def test_expected_errors_keep_their_message_but_lose_the_traceback(self):
        for exc in (UserError("You may not do that"), AccessDenied("no")):
            with self.subTest(exc=type(exc).__name__):
                exc = self._raised(type(exc), *exc.args)
                raw = self._body(exc)
                with _dev_mode([]):
                    scrubbed = lockdown.scrub_error_body(raw, exc)
                self.assertEqual(scrubbed["debug"], "")
                self.assertEqual(scrubbed["message"], raw["message"])
                self.assertEqual(scrubbed["name"], raw["name"])

    def test_developer_mode_leaves_the_body_untouched(self):
        exc = self._raised(ValueError, "x")
        raw = self._body(exc)
        with _dev_mode(["all"]):
            self.assertEqual(lockdown.scrub_error_body(raw, exc), raw)

    def test_the_dispatchers_replace_the_core_ones(self):
        self.assertIs(http._dispatchers["jsonrpc"], lockdown.HamsJsonRPCDispatcher)
        self.assertIs(http._dispatchers["json2"], lockdown.HamsJson2Dispatcher)

    def test_jsonrpc_handle_error_has_no_traceback(self):
        request = SimpleNamespace(make_json_response=lambda body, **kw: body)
        dispatcher = lockdown.HamsJsonRPCDispatcher(request)
        dispatcher.request_id = 7
        with _dev_mode([]):
            response = dispatcher.handle_error(
                self._raised(RuntimeError, "psycopg2 says relation hidden_table does not exist"))
        self.assertEqual(response["error"]["data"]["debug"], "")
        self.assertNotIn("hidden_table", json.dumps(response))
        self.assertNotIn("Traceback", json.dumps(response))
        self.assertEqual(response["id"], 7)

    def test_jsonrpc_handle_error_names_a_bad_request_as_a_400(self):
        request = SimpleNamespace(make_json_response=lambda body, **kw: body)
        dispatcher = lockdown.HamsJsonRPCDispatcher(request)
        dispatcher.request_id = 3
        with _dev_mode([]):
            response = dispatcher.handle_error(self._raised(BadRequest, "Missing required parameter: model"))
        self.assertEqual(response["error"]["code"], 400)
        self.assertIn("Missing required parameter: model", response["error"]["data"]["message"])
        self.assertEqual(response["error"]["data"]["debug"], "")

    def test_json2_handle_error_has_no_traceback_and_keeps_the_status(self):
        seen = {}

        def make(body, headers=None, status=None):
            seen.update(body=body, status=status)
            return body

        dispatcher = lockdown.HamsJson2Dispatcher(SimpleNamespace(make_json_response=make))
        with _dev_mode([]):
            dispatcher.handle_error(self._raised(RuntimeError, "boom with /srv/path/file.py"))
        self.assertEqual(seen["status"], 500)
        self.assertEqual(seen["body"]["debug"], "")
        self.assertNotIn("/srv/path", json.dumps(seen["body"]))
        with _dev_mode([]):
            dispatcher.handle_error(self._raised(UserError, "fine to show"))
        self.assertEqual(seen["body"]["message"], "fine to show")
        self.assertEqual(seen["body"]["debug"], "")


@tagged("post_install", "-at_install")
class TestDatabaseManagerIs404(HamsHttpCase):
    """Every /web/database route is a 404 outside developer mode (audit row 15)."""

    def test_pages_and_actions_are_not_found(self):
        with _dev_mode([]):
            for path in ("/web/database/manager", "/web/database/selector"):
                with self.subTest(path=path):
                    self.assertEqual(self.url_open(path).status_code, 404)
            for action in ("create", "duplicate", "drop", "backup", "restore", "change_password"):
                with self.subTest(action=action):
                    response = self.url_open(f"/web/database/{action}", data={"master_pwd": "x", "name": "y"})
                    self.assertEqual(response.status_code, 404)

    def test_the_database_list_rpc_is_not_found(self):
        with _dev_mode([]):
            response = self.opener.post(
                self.base_url() + "/web/database/list",
                json={"jsonrpc": "2.0", "method": "call", "params": {}},
            )
        self.assertNotIn("result", response.json())
        self.assertEqual(response.json()["error"]["code"], 404)

    def test_developer_mode_still_reaches_the_core_page(self):
        with _dev_mode(["all"]):
            response = self.url_open("/web/database/selector")
        self.assertNotEqual(response.status_code, 404)

    def test_a_normal_page_is_unaffected(self):
        with _dev_mode([]):
            self.assertEqual(self.url_open("/web/login").status_code, 200)


@tagged("post_install", "-at_install")
class TestJsonRpcErrorBody(HamsHttpCase):
    """A failing JSON-RPC call returns no traceback (audit row 15)."""

    def _post(self, **kwargs):
        return self.opener.post(self.base_url() + "/web/dataset/call_kw", **kwargs)

    def test_malformed_json_body(self):
        with _dev_mode([]):
            response = self._post(data="{not json", headers={"Content-Type": "application/json"})
        self.assertNotIn("Traceback", response.text)
        self.assertNotIn(".py", response.text)

    def test_session_expired_error_has_no_traceback(self):
        payload = {"jsonrpc": "2.0", "method": "call", "params": {
            "model": "res.partner", "method": "search", "args": [], "kwargs": {}}}
        with _dev_mode([]):
            response = self._post(json=payload)
        self.assertNotIn("Traceback", response.text)
        body = response.json()
        self.assertEqual(body["error"]["data"]["debug"], "")


@tagged("post_install", "-at_install")
class TestXmlrpcFaultsCarryNoTraceback(HamsHttpCase):
    """QA pass 2: `/xmlrpc/2/<service>` put the whole Python traceback in the fault string. Tests
    [@ANCHOR: hams_base:xmlrpc_fault_scrub]"""

    def _post(self, route, body):
        return self.url_open(route, data=body, headers={"Content-Type": "text/xml"})

    def test_a_malformed_body_has_no_traceback_or_path_in_the_reply(self):
        with _dev_mode([]):
            for route in ("/xmlrpc/2/common", "/xmlrpc/common"):
                response = self._post(route, "this is not xml")
                self.assertIn("Odoo Server Error", response.text, route)
                self.assertNotIn("Traceback", response.text, route)
                self.assertNotIn("/usr/lib", response.text, route)
                self.assertNotIn("File ", response.text, route)

    def test_an_unknown_method_has_no_traceback_in_the_reply(self):
        body = '<?xml version="1.0"?><methodCall><methodName>nosuchmethod</methodName><params/></methodCall>'
        with _dev_mode([]):
            response = self._post("/xmlrpc/2/common", body)
        self.assertNotIn("Traceback", response.text)
        self.assertNotIn("nosuchmethod", response.text)

    def test_a_wrong_password_keeps_its_message(self):
        body = (
            '<?xml version="1.0"?><methodCall><methodName>authenticate</methodName><params>'
            "<param><value><string>nodb</string></value></param>"
            "<param><value><string>nobody</string></value></param>"
            "<param><value><string>wrong</string></value></param>"
            "<param><value><struct/></value></param></params></methodCall>"
        )
        with _dev_mode([]):
            response = self._post("/xmlrpc/2/common", body)
        self.assertEqual(response.status_code, 200)
        self.assertIn("faultString", response.text)
        self.assertNotIn("Traceback", response.text)

    def test_a_good_call_still_works(self):
        body = '<?xml version="1.0"?><methodCall><methodName>version</methodName><params/></methodCall>'
        with _dev_mode([]):
            response = self._post("/xmlrpc/2/common", body)
        self.assertIn("server_version", response.text)
        self.assertNotIn("faultCode", response.text)
