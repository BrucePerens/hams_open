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
        # We need to mock requests.Session
        mock_session_class = self.safe_patch("odoo.addons.zero_sudo.daemon.json_rpc_client.requests.Session")
        mock_session = MagicMock()
        mock_session_class.return_value = mock_session
        
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.json.return_value = {"result": "success"}
        mock_session.post.return_value = mock_response

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)
        result = client.call("res.users", "search", [[]])

        self.assertEqual(result, "success")
        self.assertEqual(mock_session.post.call_count, 1)
        
        call_args, call_kwargs = mock_session.post.call_args
        host = os.environ.get("ODOO_HOST", "odoo")
        self.assertEqual(call_args[0], f"http://{host}:8069/json/2/res.users/search")
        payload = call_kwargs["json"]
        self.assertEqual(payload["args"], [[]])

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

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)

        # Simulate key rotation in the env file
        with open(self.env_path, "w") as f:
            f.write("ODOO_RPC_LOGIN=test_user\n")
            f.write("ODOO_RPC_KEY=rotated_key\n")

        result = client.call("res.users", "search", [[]])

        self.assertEqual(result, "healed")
        self.assertEqual(mock_session.post.call_count, 2)

    def test_missing_env_file(self):
        non_existent = os.path.join(self.test_dir, "non_existent.env")
        with self.assertRaises(FileNotFoundError):
            SecureJSONRPCClient(non_existent, self.base_url)

    def test_malformed_env_file(self):
        with open(self.env_path, "w") as f:
            f.write("WRONG_KEY=something\n")

        with self.assertRaises(ValueError):
            SecureJSONRPCClient(self.env_path, self.base_url)
