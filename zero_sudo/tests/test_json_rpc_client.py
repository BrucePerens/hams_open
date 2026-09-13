# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os
import tempfile
import shutil
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo import _
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.daemon.json_rpc_client import SecureJSONRPCClient


@tagged("post_install", "-at_install")
class TestSecureJSONRPCClient(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.test_dir = tempfile.mkdtemp()
        self.env_path = os.path.join(self.test_dir, "test.env")
        host = os.environ.get("ODOO_HOST", "odoo")
        self.base_url = f"http://{host}:8069"
        self.db_name = "test_db"

        # Create a mock env file
        with open(self.env_path, "w") as f:
            f.write("ODOO_RPC_LOGIN=test_user\n")
            f.write("ODOO_RPC_KEY=test_key\n")

    def tearDown(self):
        try:
            shutil.rmtree(self.test_dir)
        finally:
            super().tearDown()

    def test_call_success(self):
        # Tests [@ANCHOR: zero_sudo:json_rpc_client_init]

        # Tests [@ANCHOR: zero_sudo:json_rpc_client_load_credentials]
        # We need to mock requests.Session
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.json.return_value = {"result": "success"}
        mock_session.post.return_value = mock_response

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)
        result = client.call("res.users", "search", domain=[])

        self.assertEqual(result, "success")
        self.assertEqual(mock_session.post.call_count, 1)

        call_args, call_kwargs = mock_session.post.call_args
        host = os.environ.get("ODOO_HOST", "odoo")
        self.assertEqual(call_args[0], f"http://{host}:8069/json/2/res.users/search")
        payload = call_kwargs["json"]
        # bug-hunt (2026-09-13): Odoo's real JSON-2 route
        # (odoo.http.Json2Dispatcher.dispatch) merges the JSON body's own
        # top-level keys straight into the endpoint's keyword arguments --
        # there is no generic "args" envelope. A caller sends `ids` (the
        # recordset the method runs on) plus the target method's own real
        # keyword arguments directly.
        self.assertEqual(payload["ids"], [])
        self.assertEqual(payload["domain"], [])
        self.assertNotIn("args", payload)

    def test_call_sends_a_real_bearer_authorization_header(self):
        # bug-hunt (2026-09-13): the route is declared auth='bearer'
        # (odoo/addons/rpc/controllers/json2.py) -- confirmed by reading
        # ir_http.py's own _auth_method_bearer, it requires a real
        # "Authorization: Bearer <api_key>" header checked against
        # res.users.apikeys, and nothing else. The client used to send only
        # a home-grown X-Auth-Signature/X-Auth-Nonce scheme that no
        # server-side code anywhere validates, so it never authenticated at
        # all against the real endpoint.
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.json.return_value = {"result": "ok"}
        mock_session.post.return_value = mock_response

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)
        client.call("res.users", "search", domain=[])

        _call_args, call_kwargs = mock_session.post.call_args
        headers = call_kwargs["headers"]
        self.assertEqual(headers["Authorization"], "Bearer test_key")
        self.assertNotIn("X-Auth-Signature", headers)

    def test_call_self_healing(self):
        # [@ANCHOR: zero_sudo:COMM_test_call_self_healing]
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session
        
        # Responses:
        # 1. Exec fail (401)
        # 2. Exec success
        mock_exec_fail = MagicMock()
        mock_exec_fail.status_code = 401
        mock_exec_fail.json.return_value = {"error": _("Access Denied Error")}
        
        mock_exec_success = MagicMock()
        mock_exec_success.status_code = 200
        mock_exec_success.json.return_value = {"result": "healed"}

        mock_session.post.side_effect = [mock_exec_fail, mock_exec_success]

        # Tests [@ANCHOR: COMM_json_rpc_self_healing_retry]
        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)

        # Simulate key rotation in the env file
        with open(self.env_path, "w") as f:
            f.write("ODOO_RPC_LOGIN=test_user\n")
            f.write("ODOO_RPC_KEY=rotated_key\n")

        result = client.call("res.users", "search", domain=[])

        self.assertEqual(result, "healed")
        self.assertEqual(mock_session.post.call_count, 2)

        # bug-hunt (2026-09-13): prove the retry actually re-authenticates
        # with the freshly-reloaded key, not just a blind resend of the
        # identical (still-stale) request.
        _first_args, first_kwargs = mock_session.post.call_args_list[0]
        _retry_args, retry_kwargs = mock_session.post.call_args_list[1]
        self.assertEqual(first_kwargs["headers"]["Authorization"], "Bearer test_key")
        self.assertEqual(retry_kwargs["headers"]["Authorization"], "Bearer rotated_key")

    def test_call_does_not_retry_on_unrelated_error_mentioning_access_words(self):
        # bug-hunt (2026-09-13): regression test for a real false-positive
        # risk in the retry-trigger check -- a 422 ValidationError (a
        # business error a credential reload could never fix) whose own
        # message text happens to contain the word "AccessError" used to
        # also trigger the self-healing retry, via a bare substring match
        # against the whole stringified error object. Odoo's own exception
        # hierarchy (odoo/exceptions.py) already guarantees every failure a
        # credential reload COULD fix surfaces as a real HTTP 401/403, so
        # the substring match was pure risk with no real coverage -- this
        # locks in that a 422 with access-flavored TEXT makes exactly one
        # call, not two.
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session

        mock_validation_error = MagicMock()
        mock_validation_error.status_code = 422
        mock_validation_error.json.return_value = {
            "error": {
                "code": 422,
                "message": "Validation Error",
                "data": {
                    "name": "odoo.exceptions.ValidationError",
                    "message": "Quota exceeded: AccessError raised elsewhere in the stack trace",
                },
            }
        }
        mock_session.post.return_value = mock_validation_error

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)
        with self.assertRaises(RuntimeError):
            client.call("res.users", "search", domain=[])

        self.assertEqual(
            mock_session.post.call_count,
            1,
            "A 422 business error must never trigger the credential-reload "
            "retry, even when its own message text happens to mention "
            "'AccessError' -- only a real 401/403 should.",
        )

    def test_missing_env_file(self):
        non_existent = os.path.join(self.test_dir, "non_existent.env")
        with self.assertRaises(FileNotFoundError):
            SecureJSONRPCClient(non_existent, self.base_url)

    def test_malformed_env_file(self):
        with open(self.env_path, "w") as f:
            f.write("WRONG_KEY=something\n")

        with self.assertRaises(ValueError):
            SecureJSONRPCClient(self.env_path, self.base_url)
