# -*- coding: utf-8 -*-
from odoo.tests.common import TransactionCase, tagged
from odoo.exceptions import AccessError, UserError
from unittest.mock import patch, MagicMock, mock_open
import subprocess
import os
import odoo


@tagged("post_install", "-at_install")
class TestSecurityUtils(TransactionCase):

    def test_01_mechanical_secret_block_enforcement(self):
        # [@ANCHOR: test_01_mechanical_secret_block_enforcement]
        # Tests [@ANCHOR: get_system_param]
        # Tests [@ANCHOR: set_system_param]
        # Tests [@ANCHOR: story_parameter_whitelisting]
        # Tests [@ANCHOR: journey_securing_configuration]
        """Verify that parameters matching cryptographic patterns are blocked."""
        utils = self.env["zero_sudo.security.utils"]

        # Safe parameters should pass
        base_url = utils._get_system_param("web.base.url")
        self.assertTrue(base_url is not None or base_url is False)

        # Test setting safe parameter
        # Use a dummy context so it doesn't break the actual DB url
        utils._set_system_param("web.base.url", base_url)

        # Dangerous parameters MUST be violently rejected
        dangerous_keys = [
            "database.secret",
            "my_api_key",
            "admin_password",
            "oauth_token",
            "cert_file",
        ]
        for key in dangerous_keys:
            with self.assertRaises(
                AccessError,
                msg=f"Extracting dangerous param '{key}' MUST raise an AccessError.",
            ):
                utils._get_system_param(key)

            with self.assertRaises(
                AccessError,
                msg=f"Setting dangerous param '{key}' MUST raise an AccessError.",
            ):
                utils._set_system_param(key, "hack")

        # Non-whitelisted safe parameters MUST also be rejected
        with self.assertRaises(
            AccessError,
            msg="Extracting non-whitelisted param MUST raise an AccessError.",
        ):
            utils._get_system_param("some.unregistered.safe.param")

    def test_02_bdd_ormcache_query_counting_service_uid(self):
        # [@ANCHOR: test_get_service_uid]
        # Tests [@ANCHOR: get_service_uid]
        # Tests [@ANCHOR: story_secure_escalation]
        # Tests [@ANCHOR: journey_service_account_lifecycle]
        utils = self.env["zero_sudo.security.utils"]

        # We must test using a valid Service Account, as the utility
        # violently rejects human users like 'base.user_admin'
        svc_xml_id = "zero_sudo.mail_service_internal"

        # If this raises AccessError, it means the test env is missing the demo service user.
        # Ensure 'test_tours' data or zero_sudo demo data created it. Assuming it exists:
        try:
            utils._get_service_uid(svc_xml_id)
        except AccessError:
            self.skipTest(f"Service account {svc_xml_id} not available in test env.")

        with patch.object(
            self.env.cr, "execute", wraps=self.env.cr.execute
        ) as mock_execute:
            utils._get_service_uid(svc_xml_id)
            for call in mock_execute.call_args_list:
                self.assertNotIn("res_users", call[0][0])

    def test_03_bdd_event_bus_payload_generation(self):
        # [@ANCHOR: test_coherent_cache_signal]
        # Tests [@ANCHOR: coherent_cache_signal]
        # Tests [@ANCHOR: story_cache_signaling]
        utils = self.env["zero_sudo.security.utils"]
        with patch.object(self.env.cr, "execute") as mock_execute:
            utils._notify_cache_invalidation("test.model", "test_key")
            mock_execute.assert_called_once_with(
                "SELECT pg_notify(%s, %s)",
                ("cache_invalidation", "test.model:test_key"),
            )

    def test_04_god_mode_block_enforcement(self):
        """Verify that any Service Account granted base.group_system is violently rejected."""
        # 1. Create a rogue service account
        rogue_user = self.env["res.users"].create(
            {
                "name": "Rogue God Account",
                "login": "rogue_god",
                "is_service_account": True,
                "group_ids": [(4, self.env.ref("base.group_system").id)],
            }
        )

        # 2. Bind it to an XML ID so _get_service_uid can look it up
        self.env["ir.model.data"].create(
            {
                "module": "rogue_module",
                "name": "sneaky_admin_service",
                "model": "res.users",
                "res_id": rogue_user.id,
            }
        )

        utils = self.env["zero_sudo.security.utils"]
        with self.assertRaises(
            AccessError,
            msg="Must block Service Accounts with group_system from escalating privileges.",
        ):
            utils._get_service_uid("rogue_module.sneaky_admin_service")

    def test_05_notify_cache_invalidation_list(self):
        """Test _notify_cache_invalidation with a list payload."""
        utils = self.env["zero_sudo.security.utils"]
        with patch.object(self.env.cr, "execute") as mock_execute:
            utils._notify_cache_invalidation("test.model", ["key1", "key2", "key1"])

            # Extract the arguments passed to execute
            args, _ = mock_execute.call_args
            query = args[0]
            params = args[1]

            self.assertEqual(query, "SELECT pg_notify(%s, payload) FROM unnest(%s) AS payload")
            self.assertEqual(params[0], "cache_invalidation")
            # We must sort the payloads because set conversion makes the order non-deterministic
            self.assertListEqual(sorted(params[1]), sorted(["test.model:key1", "test.model:key2"]))

    def test_06_get_deterministic_hash(self):
        # [@ANCHOR: test_deterministic_hash]
        # Tests [@ANCHOR: deterministic_hash]
        # Tests [@ANCHOR: story_deterministic_hash]
        """Verify _get_deterministic_hash generates consistent integer hashes."""
        utils = self.env["zero_sudo.security.utils"]

        hash1 = utils._get_deterministic_hash("test_string_1")
        hash2 = utils._get_deterministic_hash("test_string_1")
        hash3 = utils._get_deterministic_hash("test_string_2")
        hash4 = utils._get_deterministic_hash(12345)

        self.assertIsInstance(hash1, int)
        self.assertEqual(hash1, hash2, "Same input should yield same hash")
        self.assertNotEqual(hash1, hash3, "Different inputs should yield different hashes")
        self.assertIsInstance(hash4, int, "Should handle non-string inputs gracefully")
        self.assertTrue(0 <= hash1 <= 2147483647, "Hash should be within 32-bit integer range")

    @patch("subprocess.run")
    @patch("os.path.exists")
    def test_07_update_python_venv(self, mock_exists, mock_run):
        # [@ANCHOR: test_update_python_venv]
        # Tests [@ANCHOR: update_python_venv]
        # Tests [@ANCHOR: story_venv_management]
        """Test the _update_python_venv method."""
        utils = self.env["zero_sudo.security.utils"]

        # Test 1: requirements.txt not found
        mock_exists.return_value = False
        with self.assertRaises(UserError):
            utils._update_python_venv()

        # Test 2: requirements.txt exists, subprocess succeeds
        mock_exists.return_value = True
        mock_run.return_value.returncode = 0
        self.assertTrue(utils._update_python_venv())

        # Test 3: subprocess fails
        mock_run.side_effect = subprocess.CalledProcessError(1, "cmd", stderr="pip error")
        with self.assertRaises(UserError) as cm:
            utils._update_python_venv()
        self.assertIn("pip error", str(cm.exception))

        # Test 4: AccessError for non-admin
        non_admin = self.env["res.users"].create({
            "name": "Non Admin",
            "login": "non_admin_no_groups",
        })
        with self.assertRaises(AccessError):
            utils.with_user(non_admin)._update_python_venv()

    def test_08_get_crypto_secret(self):
        # Tests [@ANCHOR: get_crypto_secret]
        """Test the cryptographic secret retrieval hierarchy."""
        utils = self.env["zero_sudo.security.utils"]

        # 1. Test environment variable resolution
        with patch.dict(os.environ, {"HAMS_CRYPTO_KEY": "test_env_key"}):
            self.assertEqual(utils._get_crypto_secret(), "test_env_key")

        # 2. Test file fallback
        with patch.dict(os.environ, {}, clear=True):
            with patch("builtins.open", mock_open(read_data="test_file_key\n")):
                self.assertEqual(utils._get_crypto_secret(), "test_file_key")

            # 3. Test configuration fallback
            with patch("builtins.open", side_effect=Exception("File not found")):
                with patch.object(odoo.tools.config, "get", return_value="test_config_key"):
                    self.assertEqual(utils._get_crypto_secret(), "test_config_key")

    def test_09_bootstrap_knowledge_docs(self):
        # [@ANCHOR: test_zero_sudo_doc_installer]
        # Tests [@ANCHOR: zero_sudo_doc_installer]
        # Tests [@ANCHOR: story_zero_sudo_doc_installer]
        # Tests [@ANCHOR: journey_developer_integration]
        """
        Verify that the _bootstrap_knowledge_docs method correctly
        discovers and installs documentation from module manifests.
        """
        article_model_name = None
        if "knowledge.article" in self.env:
            article_model_name = "knowledge.article"
        elif "manual.article" in self.env:
            article_model_name = "manual.article"

        if not article_model_name:
            self.skipTest("No documentation API available.")

        # Trigger the centralized installer
        self.env["ir.module.module"]._bootstrap_knowledge_docs()

        # Check if the primary documentation was successfully injected
        article = self.env[article_model_name].search([("name", "=", "Zero-Sudo Security Core")], limit=1)
        self.assertTrue(article, "Documentation for zero_sudo should be installed via the manifest.")

    def test_10_get_service_env(self):
        """Verify _get_service_env correctly disables tracking and prefetching."""
        utils = self.env["zero_sudo.security.utils"]
        svc_xml_id = "zero_sudo.mail_service_internal"

        try:
            expected_uid = utils._get_service_uid(svc_xml_id)
        except AccessError:
            self.skipTest(f"Service account {svc_xml_id} not available in test env.")

        env_svc = utils._get_service_env(svc_xml_id)

        # Ensure environment switches cleanly
        self.assertEqual(env_svc.user.id, expected_uid)

        # ADR-0001: Ensure background context overrides exist to prevent nested cache faults
        self.assertTrue(env_svc.context.get("mail_notrack"))
        self.assertFalse(env_svc.context.get("prefetch_fields"))

    @patch("shutil.which")
    def test_11_ensure_executable(self, mock_which):
        """Verify the fallback system for auto-installing binary manifests."""
        utils = self.env["zero_sudo.security.utils"]

        # Scenario 1: Binary exists in system PATH
        mock_which.return_value = "/usr/bin/kopia"
        self.assertEqual(utils._ensure_executable("kopia"), "/usr/bin/kopia")

        # Scenario 2: Missing binary, no manifest available (should raise UserError)
        mock_which.return_value = None
        with patch.dict(self.env.registry.models, clear=False):
            with self.assertRaises(UserError) as cm:
                utils._ensure_executable("missing_bin", pkg_name="apt-pkg-missing")
            self.assertIn("Missing dependency", str(cm.exception))
            self.assertIn("apt-pkg-missing", str(cm.exception))

        # Scenario 3: Fallback dynamically invokes the manifest downloader module
        mock_manifest = MagicMock()
        mock_manifest.ensure_executable.return_value = "/var/lib/odoo/hams_bin/kopia"
        mock_env = {"binary.manifest": mock_manifest}

        with patch.object(utils, "_get_service_env", return_value=mock_env):
            # Intercept "binary.manifest in self.env" check to return True
            with patch.object(self.env, "__contains__", return_value=True):
                res = utils._ensure_executable("kopia", svc_xml_id="zero_sudo.mail_service_internal")
                self.assertEqual(res, "/var/lib/odoo/hams_bin/kopia")
                mock_manifest.ensure_executable.assert_called_once_with("kopia")

    def test_12_kv_store(self):
        """Verify the lightweight Service Account Key-Value storage abstraction."""
        utils = self.env["zero_sudo.security.utils"]

        try:
            utils._get_service_uid("zero_sudo.odoo_facility_service_internal")
        except AccessError:
            self.skipTest("Facility Service Account missing from test suite.")

        # Test Write & Read
        utils._set_kv("test_key_1", "test_value")
        self.assertEqual(utils._get_kv("test_key_1"), "test_value")

        # Test Update
        utils._set_kv("test_key_1", "updated_value")
        self.assertEqual(utils._get_kv("test_key_1"), "updated_value")

        # Test Missing Key
        self.assertIsNone(utils._get_kv("non_existent_key"))

    def test_13_service_uid_error_paths(self):
        """Audit all rejection branches within the service account lookup logic."""
        utils = self.env["zero_sudo.security.utils"]

        # 1. Invalid XML ID Format
        with self.assertRaises(AccessError) as cm:
            utils._get_service_uid("invalid_format_no_dot")
        self.assertIn("Invalid XML ID format", str(cm.exception))

        # 2. Account Not Found
        with self.assertRaises(AccessError) as cm:
            utils._get_service_uid("base.non_existent_xml_id")
        self.assertIn("not found", str(cm.exception))

        # 3. Deny Human Admin Pass-through
        with self.assertRaises(AccessError) as cm:
            utils._get_service_uid("base.user_admin")
        self.assertIn("is a human user", str(cm.exception))

        # 4. Deny Disabled Accounts
        disabled_user = self.env["res.users"].create({
            "name": "Disabled SA",
            "login": "disabled_sa",
            "is_service_account": True,
            "active": False,
        })
        self.env["ir.model.data"].create({
            "module": "zero_sudo",
            "name": "disabled_sa_xml",
            "model": "res.users",
            "res_id": disabled_user.id,
        })

        with self.assertRaises(AccessError) as cm:
            utils._get_service_uid("zero_sudo.disabled_sa_xml")
        self.assertIn("Service Account is disabled", str(cm.exception))
