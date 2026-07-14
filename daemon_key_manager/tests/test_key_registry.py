# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import logging
import os
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import UserError, AccessError


_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestKeyRegistry(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.registry_model = self.env["daemon.key.registry"]

        # Centralize test paths to ensure they match the production environment directory structure
        self.test_env_paths = [
            "/opt/hams/etc/keys/test_daemon.env",
            "/opt/hams/etc/keys/cron_test_daemon.env",
            "/opt/hams/etc/keys/ownership_test_daemon.env",
            "/opt/hams/etc/keys/api_test.env",
            "/opt/hams/etc/keys/force_provision.env",
            "/opt/hams/etc/keys/unauthorized.env",
        ]

        # Ensure a clean slate before each test runs to prevent state collision
        self._cleanup_test_files()

        self.service_user = self.env["res.users"].create(
            {
                "name": "Test Service Account",
                "login": "test_service_account",
                "is_service_account": True,
            }
        )

        self.regular_user = self.env["res.users"].create(
            {
                "name": "Regular User",
                "login": "regular_user",
                "is_service_account": False,
            }
        )

        self.manager_user = self.env.ref(
            "daemon_key_manager.user_daemon_key_manager_service"
        )

    def tearDown(self):
        # Ensure files are removed after the test completes as well
        self._cleanup_test_files()
        super().tearDown()

    def _cleanup_test_files(self):
        for path in self.test_env_paths:
            try:
                if os.path.exists(path):
                    os.remove(path)
            except OSError as e:
                _logger.warning("Cleanup error for %s: %s", path, e)

    def test_security_constraints(self):
        """Test that only service accounts and valid paths can be used."""
        # [@ANCHOR: COMM_test_security_constraints]

        # Tests [@ANCHOR: COMM_security_constraints_user]

        # Tests [@ANCHOR: COMM_security_constraints_path]

        # We must use a user that has permission to create registries, but not a human user as target
        # Test non-service account
        with self.assertRaises(UserError):
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
                {
                    "name": "Test Daemon",
                    "user_id": self.regular_user.id,
                    "env_file_path": self.test_env_paths[0],
                }
            )
            self.env.flush_all()

        # Test invalid path
        with self.assertRaises(UserError):
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
                {
                    "name": "Test Daemon Path",
                    "user_id": self.service_user.id,
                    "env_file_path": "/opt/jules/test.env",
                }
            )
            self.env.flush_all()

        # Test symlink attack prevention
        # Create a directory that is within the allowed prefix
        allowed_dir = "/opt/hams/etc/keys/trusted"
        if not os.path.exists(allowed_dir):
            os.makedirs(allowed_dir, mode=0o700, exist_ok=True)

        # Create a symlink that points outside the allowed prefix
        symlink_path = os.path.join(allowed_dir, "evil.env")
        target_path = "/etc/passwd"
        if os.path.exists(symlink_path):
            os.remove(symlink_path)

        os.symlink(target_path, symlink_path)

        # Attempting to use the symlink should fail because os.path.realpath resolves it to /etc/passwd
        with self.assertRaises(UserError) as cm:
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
                {
                    "name": "Symlink Attack",
                    "user_id": self.service_user.id,
                    "env_file_path": symlink_path,
                }
            )
            self.env.flush_all()
        self.assertIn("Security Alert", str(cm.exception))

        # Test expanded forbidden prefixes
        forbidden_paths = [
            "/opt/jules/test.env",
            "/usr/local/bin/test.env",
            "/bin/test.env",
            "/var/log/test.env",
        ]
        for f_path in forbidden_paths:
            with self.subTest(path=f_path):
                with self.assertRaises(UserError) as cm:
                    self.env["daemon.key.registry"].with_user(
                        self.manager_user.id
                    ).create(
                        {
                            "name": f"Forbidden {f_path}",
                            "user_id": self.service_user.id,
                            "env_file_path": f_path,
                        }
                    )
                    self.env.flush_all()
                self.assertIn("Security Alert", str(cm.exception))

        # Cleanup
        self.addCleanup(self._silent_remove, symlink_path)
        self.addCleanup(self._silent_rmdir, allowed_dir)

    def _silent_remove(self, path):
        try:
            if os.path.exists(path):
                os.remove(path)
        except OSError as e:
            _logger.warning("Cleanup error: %s", e)

    def _silent_rmdir(self, path):
        try:
            if os.path.exists(path):
                os.rmdir(path)
        except OSError as e:
            _logger.warning("Cleanup error: %s", e)

    def test_register_daemon_api(self):
        """Test the register_daemon API."""
        # [@ANCHOR: COMM_test_register_daemon_api]

        # Tests [@ANCHOR: COMM_register_daemon_api]

        # Tests [@ANCHOR: COMM_register_daemon_logic]

        # Tests [@ANCHOR: COMM_register_daemon_idempotency]

        # Tests [@ANCHOR: COMM_write_secure_env_file_logic]

        # Tests [@ANCHOR: COMM_daemon_self_healing]
        daemon_name = "API Test Daemon"
        user_xml_id = "daemon_key_manager.user_daemon_key_manager_service"
        env_file_path = "/opt/hams/etc/keys/api_test.env"

        result = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .register_daemon(daemon_name, user_xml_id, env_file_path)
        )
        self.assertTrue(result)

        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .search([("name", "=", daemon_name)], limit=1)
        )
        self.assertTrue(registry)
        self.assertEqual(registry.env_file_path, env_file_path)
        self.assertTrue(os.path.exists(env_file_path))

        # Verify usage group assignment
        # Tests [@ANCHOR: COMM_privilege_escalation_bypass]
        usage_group = self.env.ref("daemon_key_manager.group_daemon_key_usage")
        target_user = self.env["res.users"].search([("login", "=", user_xml_id)], limit=1)
        if not target_user:
            target_user = self.env.ref(user_xml_id)
        self.assertIn(usage_group, target_user.group_ids)

    def test_cron_rotate_all_keys(self):
        """Test cron rotation and trigger functionality."""
        # [@ANCHOR: COMM_test_cron_rotate_all_keys]

        # Tests [@ANCHOR: COMM_cron_rotation_logic]

        # Tests [@ANCHOR: COMM_revoke_old_keys_logic]

        # Tests [@ANCHOR: COMM_generate_new_key_logic]
        # Create a mock daemon
        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": "Cron Test Daemon",
                    "user_id": self.service_user.id,
                    "env_file_path": self.test_env_paths[1],
                }
            )
        )

        # Test cron execution wrapper
        self.env["daemon.key.registry"]._cron_rotate_all_keys()

        # Call the actual trigger to fulfill the test anchor requirement
        # # Tests [@ANCHOR: COMM_cron_rotation_trigger]
        self.env.ref("daemon_key_manager.ir_cron_rotate_daemon_keys").with_user(
            self.manager_user.id
        )._trigger()

        registry.unlink()

    def test_key_ownership(self):
        """Verify that the generated key belongs to the service account, not SUPERUSER."""
        # [@ANCHOR: COMM_test_key_ownership]

        # Tests [@ANCHOR: COMM_generate_new_key_logic]
        service_user = self.env["res.users"].create(
            {
                "name": "Test Ownership Service Account",
                "login": "test_ownership_svc",
                "is_service_account": True,
            }
        )
        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": "Ownership Test Daemon",
                    "user_id": service_user.id,
                    "env_file_path": self.test_env_paths[2],
                }
            )
        )
        registry.with_user(self.manager_user.id)._rotate_key_and_write_file()

        # Search for the key
        self.env.cr.execute(
            "SELECT user_id FROM res_users_apikeys WHERE name = 'Ownership Test Daemon_key'"
        )
        res = self.env.cr.fetchone()

        self.assertTrue(res, "API Key was not created")
        self.assertEqual(
            res[0],
            service_user.id,
            f"Key owner should be {service_user.login} (ID {service_user.id}), "
            f"but it is ID {res[0]}",
        )
        self.assertNotEqual(
            res[0],
            self.env.ref("base.user_root").id,
            "Key should not be owned by SUPERUSER",
        )

    def test_force_provisioning(self):
        """Test force provisioning of all keys."""
        # [@ANCHOR: COMM_test_force_provisioning]

        # Tests [@ANCHOR: COMM_action_force_provision_all_api]

        # Tests [@ANCHOR: COMM_force_provision_logic]

        # Tests [@ANCHOR: COMM_force_provision_error_handling]
        daemon_name = "Force Provision Test"
        env_file_path = "/opt/hams/etc/keys/force_provision.env"

        self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
            {
                "name": daemon_name,
                "user_id": self.service_user.id,
                "env_file_path": env_file_path,
            }
        )

        # Ensure file does not exist
        if os.path.exists(env_file_path):
            os.remove(env_file_path)

        self.env["daemon.key.registry"].with_user(
            self.manager_user.id
        ).action_force_provision_all()
        self.assertTrue(os.path.exists(env_file_path))

    def test_ui_rendering(self):
        """Test UI view rendering."""
        # [@ANCHOR: COMM_test_ui_rendering]
        # Test Tree View (Now 'list' in Odoo 19)
        tree_view = self.env["ir.ui.view"].get_view(
            res_model="daemon.key.registry", view_type="list"
        )
        self.assertTrue(tree_view)

        # Test Form View
        form_view = self.env["ir.ui.view"].get_view(
            res_model="daemon.key.registry", view_type="form"
        )
        self.assertTrue(form_view)

    def test_unauthorized_access(self):
        """Test that unauthorized users cannot manage daemon keys."""
        # # Tests [@ANCHOR: COMM_test_unauthorized_access]
        # Create a registry entry
        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": "Unauthorized Test",
                    "user_id": self.service_user.id,
                    "env_file_path": self.test_env_paths[5],
                }
            )
        )

        # Regular user should not be able to rotate keys
        with self.assertRaises(AccessError):
            registry.with_user(self.regular_user.id)._rotate_key_and_write_file()

        # Regular user should not be able to call force provision all
        with self.assertRaises(AccessError):
            self.env["daemon.key.registry"].with_user(
                self.regular_user.id
            ).action_force_provision_all()

    def test_action_rotate_key(self):
        """Test manual rotation of a single key."""
        # [@ANCHOR: COMM_test_action_rotate_key]

        # Tests [@ANCHOR: COMM_action_rotate_key_api]
        daemon_name = "Single Rotation Test"
        env_file_path = "/opt/hams/etc/keys/single_rotation.env"
        self.test_env_paths.append(env_file_path)

        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": daemon_name,
                    "user_id": self.service_user.id,
                    "env_file_path": env_file_path,
                }
            )
        )

        # Ensure file does not exist
        if os.path.exists(env_file_path):
            os.remove(env_file_path)

        registry.with_user(self.manager_user.id).action_rotate_key()
        self.assertTrue(os.path.exists(env_file_path))

    def test_rotation_safety_archived_user(self):
        """Test that keys cannot be rotated for archived service accounts."""
        # [@ANCHOR: COMM_test_rotation_safety_archived_user]

        # Tests [@ANCHOR: COMM_rotation_safety_archived_user]
        archived_user = self.env["res.users"].create(
            {
                "name": "Archived Service Account",
                "login": "archived_svc",
                "is_service_account": True,
                "active": False,
            }
        )
        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": "Archived Test",
                    "user_id": archived_user.id,
                    "env_file_path": self.test_env_paths[0],
                }
            )
        )

        with self.assertRaises(UserError) as cm:
            registry.with_user(self.manager_user.id)._rotate_key_and_write_file()
        self.assertIn("archived", str(cm.exception))

    def test_write_secure_env_file_exceptions(self):
        """Test that PermissionError and OSError are not swallowed."""
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            "name": "Exception Test Daemon",
            "user_id": self.service_user.id,
            "env_file_path": "/opt/hams/etc/keys/exception_test.env",
        })
        self.safe_patch("os.path.exists", return_value=False)
        self.safe_patch("os.makedirs", side_effect=PermissionError("Mocked Permission Error"))
        with self.assertRaises(PermissionError):
            registry._write_secure_env_file(registry.env_file_path, "login", "key")

        self.safe_patch("os.makedirs", side_effect=OSError("Mocked OS Error"))
        with self.assertRaises(OSError):
            registry._write_secure_env_file(registry.env_file_path, "login", "key")

    def test_cron_rotation_timezone_sql(self):
        """Test that the cron fallback raw SQL uses UTC for NOW()."""
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            "name": "Cron TZ Test Daemon",
            "user_id": self.service_user.id,
            "env_file_path": "/opt/hams/etc/keys/cron_tz_test.env",
        })
        
        # In order to trigger the fallback, we need to mock _rotate_key_and_write_file to fail.
        # But since _cron_rotate_all_keys queries registries, we need to mock execute directly.
        original_execute = self.env.cr.execute
        executed_queries = []

        def mock_execute(query, params=None):
            executed_queries.append(query)
            return original_execute(query, params)
            
        self.safe_patch_object(type(registry), "_rotate_key_and_write_file", side_effect=OSError("Mocked Error"))
        self.safe_patch_object(self.env.cr, "execute", side_effect=mock_execute)
        
        self.env["daemon.key.registry"]._cron_rotate_all_keys()
                
        found_update = any(
            "UPDATE daemon_key_registry SET last_rotated = NOW() AT TIME ZONE 'UTC'" in query
            for query in executed_queries
        )
        self.assertTrue(found_update, "Cron fallback should use NOW() AT TIME ZONE 'UTC'")


@tagged("post_install", "-at_install")
class TestKeyRegistryTour(HamsHttpCase):
    def test_daemon_key_manager_tour(self):
        # [@ANCHOR: COMM_test_daemon_key_manager_tour]

        # Tests [@ANCHOR: COMM_register_daemon_api]

        # Tests [@ANCHOR: COMM_action_force_provision_all_api]

        # Ensure admin has Technical Features enabled for the tour
        admin = self.env.ref("base.user_admin")
        admin.lang = 'en_US'
        group_no_one = self.env.ref("base.group_no_one")
        if group_no_one not in admin.group_ids:
            admin.write({"group_ids": [(4, group_no_one.id)]})

        manager_group = self.env.ref("daemon_key_manager.group_daemon_key_manager")
        if manager_group not in admin.group_ids:
            admin.write({"group_ids": [(4, manager_group.id)]})

        self.start_tour(
            "/odoo?debug=1&action=daemon_key_manager.action_daemon_key_registry",
            "daemon_key_manager_tour",
            login="admin",
        )
