import unittest
from unittest.mock import patch, MagicMock
import os
import tempfile
import shutil

from json_rpc_client import SecureJSONRPCClient

class TestSecureJSONRPCClient(unittest.TestCase):

    def setUp(self):
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

    @patch("requests.post")
    def test_call_success(self, mock_post):
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

    @patch("requests.post")
    def test_call_self_healing(self, mock_post):
        # Setup mock responses: first failure (401), then success
        mock_fail = MagicMock()
        mock_fail.status_code = 401
        mock_fail.json.return_value = {"error": "Access Denied"}

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

if __name__ == "__main__":
    unittest.main()
