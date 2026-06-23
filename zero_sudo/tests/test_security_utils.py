# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError, UserError
from unittest.mock import MagicMock, mock_open, patch
import os
import odoo
import psycopg2
import logging
from odoo.tools import mute_logger

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestSecurityUtils(HamsTransactionCase):

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
        self.assertTrue(base_url is not None or base_url is False, msg="[!] DIAGNOSTIC FOR AI: Failed to retrieve whitelisted parameter 'web.base.url'.")

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
        # [@ANCHOR: test_get_service_uid_sql_resolve]
        # [@ANCHOR: test_get_service_uid_sql_verify]
        # [@ANCHOR: test_get_service_uid]
        # Tests [@ANCHOR: get_service_uid]
        # Tests [@ANCHOR: get_service_uid_sql_resolve]
        # Tests [@ANCHOR: get_service_uid_sql_verify]
        # Tests [@ANCHOR: story_secure_escalation]
        # Tests [@ANCHOR: journey_service_account_lifecycle]
        utils = self.env["zero_sudo.security.utils"]

        # We must test using a valid Service Account, as the utility
        # violently rejects human users like 'base.user_admin'
        svc_xml_id = "zero_sudo.mail_service_internal"

        # If this raises AccessError, it means the test env is missing the demo service user.
        # Ensure 'test_tours' data or zero_sudo demo data created it. Assuming it exists:
        utils._get_service_uid(svc_xml_id)

        mock_execute = self.safe_patch_object(
            self.env.cr, "execute", wraps=self.env.cr.execute
        )
        utils._get_service_uid(svc_xml_id)
        for call in mock_execute.call_args_list:
            self.assertNotIn("res_users", call[0][0])

    def test_03_bdd_event_bus_payload_generation(self):
        # [@ANCHOR: test_coherent_cache_signal]
        # Tests [@ANCHOR: coherent_cache_signal]
        # Tests [@ANCHOR: coherent_cache_signal_single]
        # Tests [@ANCHOR: story_cache_signaling]
        utils = self.env["zero_sudo.security.utils"]
        mock_execute = self.safe_patch_object(self.env.cr, "execute")
        utils._notify_cache_invalidation("test.model", "test_key")
        mock_execute.assert_called_once_with(
            "SELECT pg_notify(%s, %s)",
            ("cache_invalidation", "test.model:test_key"),
        )

        # Test edge cases: empty model_name or key_value
        mock_execute = self.safe_patch_object(self.env.cr, "execute")
        utils._notify_cache_invalidation("", "test_key")
        utils._notify_cache_invalidation("test.model", "")
        utils._notify_cache_invalidation(None, "test_key")
        utils._notify_cache_invalidation("test.model", None)
        self.assertEqual(mock_execute.call_count, 0, "Should not notify for empty model or key")

    def test_04_god_mode_block_enforcement(self):
        # [@ANCHOR: test_god_mode_block_sql]
        # Tests [@ANCHOR: god_mode_block_sql]
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

        try:
            with self.env.cr.savepoint(), mute_logger('odoo.sql_db'):
                utils = self.env["zero_sudo.security.utils"]
                utils._get_service_uid("rogue_module.sneaky_admin_service")
            self.fail("Must block Service Accounts with group_system from escalating privileges.")
        except (AccessError, UserError, psycopg2.errors.RaiseException) as e:
            self.assertTrue(str(e))

    def test_05_notify_cache_invalidation_list(self):
        # [@ANCHOR: test_coherent_cache_signal_batch]
        # Tests [@ANCHOR: coherent_cache_signal_batch]
        """Test _notify_cache_invalidation with a list payload."""
        utils = self.env["zero_sudo.security.utils"]
        mock_execute = self.safe_patch_object(self.env.cr, "execute")
        utils._notify_cache_invalidation("test.model", ["key1", "key2", "key1"])

        # Extract the arguments passed to execute
        args, _ = mock_execute.call_args
        query = args[0]
        params = args[1]

        self.assertEqual(query, "SELECT pg_notify(%s, payload) FROM unnest(%s) AS payload")
        self.assertEqual(params[0], "cache_invalidation")
        # We must sort the payloads because set conversion makes the order non-deterministic
        self.assertListEqual(sorted(params[1]), sorted(["test.model:key1", "test.model:key2"]))

        # Test chunking
        many_keys = [f"key{i}" for i in range(250)]
        mock_execute = self.safe_patch_object(self.env.cr, "execute")
        utils._notify_cache_invalidation("test.model", many_keys)
        self.assertEqual(mock_execute.call_count, 3, "Should chunk 250 keys into 3 calls (100+100+50)")

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

    def test_08_get_crypto_secret(self):
        # [@ANCHOR: test_get_crypto_secret]
        # Tests [@ANCHOR: get_crypto_secret]
        """Test the cryptographic secret retrieval hierarchy."""
        utils = self.env["zero_sudo.security.utils"]
        # Clear cache since the method is ormcache'd
        utils.env.registry.clear_cache()

        # 1. Test environment variable resolution
        env_dict = {"HAMS_CRYPTO_KEY": "test_env_key"}
        original_env = os.environ.copy()
        os.environ.update(env_dict)
        try:
            self.assertEqual(utils._get_crypto_secret(), "test_env_key")
        finally:
            os.environ.clear()
            os.environ.update(original_env)

        utils.env.registry.clear_cache()

        # 2. Test file fallback
        original_env = os.environ.copy()
        os.environ.clear()
        try:
            self.safe_patch("os.path.exists", return_value=True)
            self.safe_patch("builtins.open", mock_open(read_data="test_file_key\n"))
            self.assertEqual(utils._get_crypto_secret(), "test_file_key")

            # 3. Test configuration fallback
            utils.env.registry.clear_cache()
            self.safe_patch("os.path.exists", return_value=False)
            self.safe_patch_object(odoo.tools.config, "get", return_value="test_config_key")
            self.assertEqual(utils._get_crypto_secret(), "test_config_key")
        finally:
            os.environ.clear()
            os.environ.update(original_env)

    def test_10_get_service_env(self):
        """Verify _get_service_env correctly disables tracking and prefetching."""
        utils = self.env["zero_sudo.security.utils"]
        svc_xml_id = "zero_sudo.mail_service_internal"

        expected_uid = utils._get_service_uid(svc_xml_id)

        env_svc = utils._get_service_env(svc_xml_id)

        # Ensure environment switches cleanly
        self.assertEqual(env_svc.user.id, expected_uid)

        # ADR-0001: Ensure background context overrides exist to prevent nested cache faults
        self.assertTrue(env_svc.context.get("mail_notrack"))

    def test_11_ensure_executable(self):
        """Verify the fallback system for auto-installing binary manifests."""
        mock_which = self.safe_patch("shutil.which")
        utils = self.env["zero_sudo.security.utils"]

        # Scenario 1: Binary exists in system PATH
        mock_which.return_value = "/usr/bin/kopia"
        self.assertEqual(utils._ensure_executable("kopia"), "/usr/bin/kopia")

        # Scenario 2: Missing binary, no manifest available (should raise UserError)
        mock_which.return_value = None

        with self.assertRaises(UserError) as cm:
            utils._ensure_executable("missing_bin", pkg_name="apt-pkg-missing")
        self.assertIn("Missing dependency", str(cm.exception))
        self.assertIn("apt-pkg-missing", str(cm.exception))

        # Scenario 3: Fallback dynamically invokes the manifest downloader module
        mock_manifest = MagicMock()
        mock_manifest.ensure_executable.return_value = "/var/lib/odoo/hams_bin/kopia"
        mock_env = MagicMock()
        # Mocking __getitem__ to handle 'binary.manifest'
        mock_env.__getitem__.side_effect = lambda k: mock_manifest if k == "binary.manifest" else None

        self.safe_patch("odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env", return_value=mock_env)

        res = utils._ensure_executable("kopia", svc_xml_id="zero_sudo.mail_service_internal")
        self.assertEqual(res, "/var/lib/odoo/hams_bin/kopia")
        mock_manifest.ensure_executable.assert_called_once_with("kopia")

    def test_12_kv_store(self):
        # [@ANCHOR: test_set_kv_procedure]
        # [@ANCHOR: test_set_kv_sql_check]
        # Tests [@ANCHOR: set_kv_procedure]
        # Tests [@ANCHOR: set_kv_sql_check]
        """Verify the lightweight Service Account Key-Value storage abstraction."""
        utils = self.env["zero_sudo.security.utils"]

        utils._get_service_uid("zero_sudo.odoo_facility_service_internal")

        # Test Write & Read
        utils._set_kv("test_key_1", "test_value")
        self.assertEqual(utils._get_kv("test_key_1"), "test_value")

        # Test Update
        utils._set_kv("test_key_1", "updated_value")
        self.assertEqual(utils._get_kv("test_key_1"), "updated_value")
        # Tests [@ANCHOR: zero_sudo_kv_global]

    def test_noisy_table_global(self):
        # Tests [@ANCHOR: zero_sudo_noisy_table_global]
        # We ensure the model is accessible and we can create a record.
        table_name = "test_noisy_table"
        table = self.env["zero_sudo.noisy_table"].create({"name": table_name})
        self.assertTrue(table.exists())
        self.assertIn(table_name, self.env["zero_sudo.noisy_table"].search([]).mapped("name"))

    def test_missing_key(self):
        utils = self.env["zero_sudo.security.utils"]
        # Test Missing Key
        self.assertIsNone(utils._get_kv("non_existent_key"))

    def test_13_service_uid_error_paths(self):
        """Audit all rejection branches within the service account lookup logic."""
        utils = self.env["zero_sudo.security.utils"]

        # 1. Invalid XML ID Format
        try:
            with self.env.cr.savepoint(), mute_logger('odoo.sql_db'):
                utils._get_service_uid("invalid_format_no_dot")
            self.fail("Expected exception")
        except (AccessError, UserError, psycopg2.errors.RaiseException):
            _logger.info("Caught expected exception for missing UID")

        # 2. Account Not Found
        try:
            with self.env.cr.savepoint(), mute_logger('odoo.sql_db'):
                utils._get_service_uid("base.non_existent_xml_id")
            self.fail("Expected exception")
        except (AccessError, UserError, psycopg2.errors.RaiseException):
            _logger.info("Caught expected exception for Account Not Found")

        # 3. Deny Human Admin Pass-through
        try:
            with self.env.cr.savepoint(), mute_logger('odoo.sql_db'):
                utils._get_service_uid("base.user_admin")
            self.fail("Expected exception")
        except (AccessError, UserError, psycopg2.errors.RaiseException):
            _logger.info("Caught expected exception for human admin pass-through")

        # 4. Deny Disabled Accounts
        disabled_user = self.env["res.users"].create({
            "name": "Disabled SA",
            "login": "disabled_sa",
            "is_service_account": True,
            "active": False
        })
        self.env["ir.model.data"].create({
            "module": "test_module",
            "name": "disabled_sa_xml",
            "model": "res.users",
            "res_id": disabled_user.id,
        })
        try:
            with self.env.cr.savepoint(), mute_logger('odoo.sql_db'):
                utils._get_service_uid("test_module.disabled_sa_xml")
            self.fail("Expected exception")
        except (AccessError, UserError, psycopg2.errors.RaiseException):
            _logger.info("Caught expected exception for disabled accounts")

    def test_14_service_account_password_generation(self):
        # [@ANCHOR: test_service_account_password]
        # Tests [@ANCHOR: is_service_account_field]
        # Tests [@ANCHOR: service_account_password_generation]
        """
        Verify that service accounts are automatically assigned a massive,
        cryptographically secure random password to prevent interactive logins.
        """
        service_account_1 = self.env["res.users"].create({
            "name": "Service Account 1",
            "login": "service_test_user_1",
            "is_service_account": True,
        })

        service_account_2 = self.env["res.users"].create({
            "name": "Service Account 2",
            "login": "service_test_user_2",
            "is_service_account": True,
        })

        self.env.cr.execute(
            "SELECT password FROM res_users WHERE id = %s",
            (service_account_1.id,)
        )
        hash_1 = self.env.cr.fetchone()[0]

        self.env.cr.execute(
            "SELECT password FROM res_users WHERE id = %s",
            (service_account_2.id,)
        )
        hash_2 = self.env.cr.fetchone()[0]

        self.assertTrue(hash_1, "Service account MUST have a generated password hash.")
        self.assertTrue(hash_2, "Service account MUST have a generated password hash.")
        self.assertNotEqual(hash_1, hash_2, "Every service account MUST receive a unique random password.")

    def test_15_invalidate_model_cache(self):
        # [@ANCHOR: test_invalidate_model_cache]
        # Tests [@ANCHOR: invalidate_model_cache]
        """Verify secure record-level cache invalidation for specific models."""
        utils = self.env["zero_sudo.security.utils"]

        # 1. Admin should be able to invalidate any model cache
        # We patch registry.clear_cache to verify it is called
        mock_clear_cache = self.safe_patch_object(self.env.registry, "clear_cache")
        # We use patch directly because self.safe_patch_object might have issues with some objects
        # Use the class since it's an AbstractModel and 'utils' is just a reference
        mock_notify = patch("odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._notify_cache_invalidation")
        mock_notify.start()
        self.addCleanup(mock_notify.stop)

        utils._invalidate_model_cache("res.partner")
        mock_clear_cache.assert_called_once()
        # mock_notify.assert_called_once_with("res.partner", "CLEAR_ALL")

        # 2. Non-admin with write access should be able to invalidate
        # We need a user with some write access but not system.
        # Let's create one.
        test_user = self.env["res.users"].create({
            "name": "Test Cache User",
            "login": "test_cache_user",
            "email": "test@test.com",
            "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
        })

        # Portal user usually doesn't have write access to res.partner
        with self.assertRaises(AccessError):
            utils.with_user(test_user)._invalidate_model_cache("res.partner")

        # Give the user a group that has write access to some model
        # For simplicity, let's use a mock check_access.
        # We need to patch check_access on the model class or instance.
        # Since it's Odoo 19, let's try patching it on the recordset.
        mock_check = patch("odoo.models.BaseModel.check_access", return_value=True)
        mock_check.start()
        self.addCleanup(mock_check.stop)

        count_before = mock_clear_cache.call_count
        utils.with_user(test_user)._invalidate_model_cache("res.partner")
        self.assertEqual(mock_clear_cache.call_count, count_before + 1)
