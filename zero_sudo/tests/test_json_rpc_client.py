# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os
import tempfile
import shutil
from unittest.mock import MagicMock
from odoo import _
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.daemon.json_rpc_client import SecureJSONRPCClient


@tagged("post_install", "-at_install")
class TestSecureJSONRPCClient(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.test_dir = tempfile.mkdtemp()
        self.env_path = os.path.join(self.test_dir, "test.env")
        self.base_url = "http://odoo:8069"
        self.db_name = "test_db"

        # Create a mock env file
        with open(self.env_path, "w") as f:
            f.write("ODOO_RPC_LOGIN=test_user\n")
            f.write("ODOO_RPC_KEY=test_key\n")

    def tearDown(self):
        shutil.rmtree(self.test_dir)
        super().tearDown()

    def test_call_success(self):
        mock_post = self.safe_patch(
            "odoo.addons.zero_sudo.daemon.json_rpc_client.requests.post"
        )
        # Setup mock response
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.json.return_value = {"result": "success"}
        mock_post.return_value = mock_response

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)
        result = client.call("res.users", "search", [[]])

        self.assertEqual(result, "success")
        mock_post.assert_called_once()
        args, kwargs = mock_post.call_args
        self.assertEqual(args[0], "http://odoo:8069/jsonrpc")
        payload = kwargs["json"]
        self.assertEqual(payload["params"]["args"][1], "test_user")
        self.assertEqual(payload["params"]["args"][2], "test_key")

    def test_call_self_healing(self):
        mock_post = self.safe_patch(
            "odoo.addons.zero_sudo.daemon.json_rpc_client.requests.post"
        )
        # Setup mock responses: first failure (401), then success
        mock_fail = MagicMock()
        mock_fail.status_code = 401
        mock_fail.json.return_value = {"error": _("Access Denied")}

        mock_success = MagicMock()
        mock_success.status_code = 200
        mock_success.json.return_value = {"result": "healed"}

        mock_post.side_effect = [mock_fail, mock_success]

        client = SecureJSONRPCClient(self.env_path, self.base_url, self.db_name)

        # Simulate key rotation in the env file
        with open(self.env_path, "w") as f:
            f.write("ODOO_RPC_LOGIN=test_user\n")
            f.write("ODOO_RPC_KEY=rotated_key\n")

        result = client.call("res.users", "search", [[]])

        self.assertEqual(result, "healed")
        self.assertEqual(mock_post.call_count, 2)

        # Check that the second call used the new key
        last_payload = mock_post.call_args_list[1][1]["json"]
        self.assertEqual(last_payload["params"]["args"][2], "rotated_key")

    def test_missing_env_file(self):
        non_existent = os.path.join(self.test_dir, "non_existent.env")
        with self.assertRaises(FileNotFoundError):
            SecureJSONRPCClient(non_existent, self.base_url)

    def test_malformed_env_file(self):
        with open(self.env_path, "w") as f:
            f.write("WRONG_KEY=something\n")

        with self.assertRaises(ValueError):
            SecureJSONRPCClient(self.env_path, self.base_url)
