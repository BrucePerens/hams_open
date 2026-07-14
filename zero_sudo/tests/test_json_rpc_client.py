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
        # We need to mock requests.Session
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session
        
        # Setup mock responses for authenticate and execute_kw
        mock_auth_response = MagicMock()
        mock_auth_response.status_code = 200
        mock_auth_response.json.return_value = {"result": 42}
        
        mock_exec_response = MagicMock()
        mock_exec_response.status_code = 200
        mock_exec_response.json.return_value = {"result": "success"}
        
        mock_session.post.side_effect = [mock_auth_response, mock_exec_response]

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)
        result = client.call("res.users", "search", [[]])

        self.assertEqual(result, "success")
        self.assertEqual(mock_session.post.call_count, 2)
        
        auth_call_args, auth_call_kwargs = mock_session.post.call_args_list[0]
        host = os.environ.get("ODOO_HOST", "odoo")
        self.assertEqual(auth_call_args[0], f"http://{host}:8069/jsonrpc")
        auth_payload = auth_call_kwargs["json"]
        self.assertEqual(auth_payload["params"]["method"], "authenticate")
        
        exec_call_args, exec_call_kwargs = mock_session.post.call_args_list[1]  # audit-ignore-i18n: Tested by [@ANCHOR: zero_sudo:COMM_test_call_self_healing]
        host = os.environ.get("ODOO_HOST", "odoo")
        self.assertEqual(exec_call_args[0], f"http://{host}:8069/jsonrpc")
        exec_payload = exec_call_kwargs["json"]
        self.assertEqual(exec_payload["params"]["method"], "execute_kw")
        self.assertEqual(exec_payload["params"]["args"][1], 42)
        self.assertEqual(exec_payload["params"]["args"][2], "test_key")

    def test_call_self_healing(self):
        # [@ANCHOR: zero_sudo:COMM_test_call_self_healing]
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session
        
        # Responses:
        # 1. Auth success
        # 2. Exec fail (401)
        # 3. Auth success (after reload)
        # 4. Exec success
        mock_auth1 = MagicMock()
        mock_auth1.status_code = 200
        mock_auth1.json.return_value = {"result": 42}
        
        mock_exec_fail = MagicMock()
        mock_exec_fail.status_code = 401
        mock_exec_fail.json.return_value = {"error": "Access Denied Error"}
        
        mock_auth2 = MagicMock()
        mock_auth2.status_code = 200
        mock_auth2.json.return_value = {"result": 42}

        mock_exec_success = MagicMock()
        mock_exec_success.status_code = 200
        mock_exec_success.json.return_value = {"result": "healed"}

        mock_session.post.side_effect = [mock_auth1, mock_exec_fail, mock_auth2, mock_exec_success]

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)

        # Simulate key rotation in the env file
        with open(self.env_path, "w") as f:
            f.write("ODOO_RPC_LOGIN=test_user\n")
            f.write("ODOO_RPC_KEY=rotated_key\n")

        result = client.call("res.users", "search", [[]])

        self.assertEqual(result, "healed")
        self.assertEqual(mock_session.post.call_count, 4)

        # Check that the last exec call used the new key
        last_exec_payload = mock_session.post.call_args_list[3][1]["json"]
        self.assertEqual(last_exec_payload["params"]["args"][2], "rotated_key")

    def test_missing_env_file(self):
        non_existent = os.path.join(self.test_dir, "non_existent.env")
        with self.assertRaises(FileNotFoundError):
            SecureJSONRPCClient(non_existent, self.base_url)

    def test_malformed_env_file(self):
        with open(self.env_path, "w") as f:
            f.write("WRONG_KEY=something\n")

        with self.assertRaises(ValueError):
            SecureJSONRPCClient(self.env_path, self.base_url)
