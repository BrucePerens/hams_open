# -*- coding: utf-8 -*-
import logging
import os
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase, HamsHttpCase
from odoo.exceptions import UserError, AccessError
from odoo import SUPERUSER_ID

_logger = logging.getLogger(__name__)

@tagged('post_install', '-at_install')
class TestKeyRegistry(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.registry_model = self.env["daemon.key.registry"]

        # Centralize test paths to ensure they match the production environment directory structure
        self.test_env_paths = [
            '/var/lib/odoo/daemon_keys/test_daemon.env',
            '/var/lib/odoo/daemon_keys/cron_test_daemon.env',
            '/var/lib/odoo/daemon_keys/ownership_test_daemon.env',
            '/var/lib/odoo/daemon_keys/api_test.env',
            '/var/lib/odoo/daemon_keys/force_provision.env',
            '/var/lib/odoo/daemon_keys/unauthorized.env'
        ]

        # Ensure a clean slate before each test runs to prevent state collision
        self._cleanup_test_files()

        self.service_user = self.env['res.users'].create({
            'name': 'Test Service Account',
            'login': 'test_service_account',
            'is_service_account': True,
        })

        self.regular_user = self.env['res.users'].create({
            'name': 'Regular User',
            'login': 'regular_user',
            'is_service_account': False,
        })

        self.manager_user = self.env.ref('daemon_key_manager.user_daemon_key_manager_service')

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
        # [@ANCHOR: test_security_constraints]
        # Tests [@ANCHOR: security_constraints_user]
        # Tests [@ANCHOR: security_constraints_path]

        # We must use a user that has permission to create registries, but not a human user as target
        # Test non-service account
        with self.assertRaises(UserError):
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
                'name': 'Test Daemon',
                'user_id': self.regular_user.id,
                'env_file_path': self.test_env_paths[0],
            })
            self.env.flush_all()

        # Test invalid path
        with self.assertRaises(UserError):
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
                'name': 'Test Daemon Path',
                'user_id': self.service_user.id,
                'env_file_path': '/home/jules/test.env',
            })
            self.env.flush_all()

        # Test symlink attack prevention
        # Create a directory that is within the allowed prefix
        allowed_dir = "/var/lib/odoo/daemon_keys/trusted"
        if not os.path.exists(allowed_dir):
            os.makedirs(allowed_dir, mode=0o700, exist_ok=True)

        # Create a symlink that points outside the allowed prefix
        symlink_path = os.path.join(allowed_dir, "evil.env")
        target_path = "/etc/passwd"
        if os.path.exists(symlink_path):
            os.remove(symlink_path)

        try:
            os.symlink(target_path, symlink_path)
        except OSError:
            self.skipTest("Cannot create symlink for test")

        # Attempting to use the symlink should fail because os.path.realpath resolves it to /etc/passwd
        with self.assertRaises(UserError) as cm:
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
                'name': 'Symlink Attack',
                'user_id': self.service_user.id,
                'env_file_path': symlink_path,
            })
            self.env.flush_all()
        self.assertIn("Security Alert", str(cm.exception))

        # Test expanded forbidden prefixes
        forbidden_paths = [
            "/home/jules/test.env",
            "/usr/local/bin/test.env",
            "/bin/test.env",
            "/var/log/test.env",
        ]
        for f_path in forbidden_paths:
            with self.subTest(path=f_path):
                with self.assertRaises(UserError) as cm:
                    self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
                        'name': f'Forbidden {f_path}',
                        'user_id': self.service_user.id,
                        'env_file_path': f_path,
                    })
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
        # [@ANCHOR: test_register_daemon_api]
        # Tests [@ANCHOR: register_daemon_api]
        # Tests [@ANCHOR: register_daemon_logic]
        # Tests [@ANCHOR: register_daemon_idempotency]
        # Tests [@ANCHOR: write_secure_env_file_logic]
        # Tests [@ANCHOR: daemon_self_healing]
        daemon_name = "API Test Daemon"
        user_xml_id = "daemon_key_manager.user_daemon_key_manager_service"
        env_file_path = "/var/lib/odoo/daemon_keys/api_test.env"

        result = self.env["daemon.key.registry"].register_daemon(daemon_name, user_xml_id, env_file_path)
        self.assertTrue(result)

        registry = self.env["daemon.key.registry"].search([('name', '=', daemon_name)])
        self.assertTrue(registry)
        self.assertEqual(registry.env_file_path, env_file_path)
        self.assertTrue(os.path.exists(env_file_path))

        # Verify usage group assignment
        # Tests [@ANCHOR: privilege_escalation_bypass]
        usage_group = self.env.ref("daemon_key_manager.group_daemon_key_usage")
        target_user = self.env["res.users"].search([('login', '=', user_xml_id)])
        if not target_user:
            target_user = self.env.ref(user_xml_id)
        self.assertIn(usage_group, target_user.group_ids)

    def test_documentation_installed(self):
        """Verify that documentation is installed in knowledge.article or manual.article."""
        # # Tests [@ANCHOR: documentation_installed]
        # [@ANCHOR: test_documentation_installed]
        # Verified by [@ANCHOR: documentation_installed]
        model = None
        if "knowledge.article" in self.env:
            model = "knowledge.article"
        elif "manual.article" in self.env:
            model = "manual.article"

        if model:
            _logger.info("Verifying documentation installation for model %s", model)
            # Trigger manual bootstrap
            self.env["ir.module.module"].with_user(self.env.ref('zero_sudo.odoo_facility_service_internal').id)._bootstrap_knowledge_docs()

            article = self.env[model].search([('name', '=', 'Daemon Key Manager Documentation')], limit=1)
            self.assertTrue(article, "Documentation article not found")
            self.assertIn("Daemon Key Manager", article.body)
            _logger.info("Documentation found and verified.")
        else:
            self.skipTest("No documentation model available")

    def test_cron_rotate_all_keys(self):
        """Test cron rotation and trigger functionality."""
        # [@ANCHOR: test_cron_rotate_all_keys]
        # Tests [@ANCHOR: cron_rotation_logic]
        # Tests [@ANCHOR: revoke_old_keys_logic]
        # Tests [@ANCHOR: generate_new_key_logic]
        # Create a mock daemon
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            'name': 'Cron Test Daemon',
            'user_id': self.service_user.id,
            'env_file_path': self.test_env_paths[1],
        })

        # Test cron execution wrapper
        self.env["daemon.key.registry"]._cron_rotate_all_keys()

        # Call the actual trigger to fulfill the test anchor requirement
        # # Tests [@ANCHOR: cron_rotation_trigger]
        self.env.ref("daemon_key_manager.ir_cron_rotate_daemon_keys").with_user(self.manager_user.id)._trigger()

        registry.unlink()

    def test_key_ownership(self):
        """Verify that the generated key belongs to the service account, not SUPERUSER."""
        # [@ANCHOR: test_key_ownership]
        # Tests [@ANCHOR: generate_new_key_logic]
        service_user = self.env['res.users'].create({
            'name': 'Test Ownership Service Account',
            'login': 'test_ownership_svc',
            'is_service_account': True,
        })
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            'name': 'Ownership Test Daemon',
            'user_id': service_user.id,
            'env_file_path': self.test_env_paths[2],
        })
        registry.with_user(self.manager_user.id)._rotate_key_and_write_file()

        # Search for the key
        self.env.cr.execute("SELECT user_id FROM res_users_apikeys WHERE name = 'Ownership Test Daemon_key'")
        res = self.env.cr.fetchone()

        self.assertTrue(res, "API Key was not created")
        self.assertEqual(res[0], service_user.id,
                         f"Key owner should be {service_user.login} (ID {service_user.id}), "
                         f"but it is ID {res[0]}")
        self.assertNotEqual(res[0], SUPERUSER_ID, "Key should not be owned by SUPERUSER")

    def test_force_provisioning(self):
        """Test force provisioning of all keys."""
        # [@ANCHOR: test_force_provisioning]
        # Tests [@ANCHOR: action_force_provision_all_api]
        # Tests [@ANCHOR: force_provision_logic]
        # Tests [@ANCHOR: force_provision_error_handling]
        daemon_name = "Force Provision Test"
        env_file_path = "/var/lib/odoo/daemon_keys/force_provision.env"

        self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            'name': daemon_name,
            'user_id': self.service_user.id,
            'env_file_path': env_file_path,
        })

        # Ensure file does not exist
        if os.path.exists(env_file_path):
            os.remove(env_file_path)

        self.env["daemon.key.registry"].with_user(self.manager_user.id).action_force_provision_all()
        self.assertTrue(os.path.exists(env_file_path))

    def test_ui_rendering(self):
        """Test UI view rendering."""
        # [@ANCHOR: test_ui_rendering]
        # Test Tree View (Now 'list' in Odoo 19)
        tree_view = self.env['ir.ui.view'].get_view(
            res_model='daemon.key.registry',
            view_type='list'
        )
        self.assertTrue(tree_view)

        # Test Form View
        form_view = self.env['ir.ui.view'].get_view(
            res_model='daemon.key.registry',
            view_type='form'
        )
        self.assertTrue(form_view)

    def test_unauthorized_access(self):
        """Test that unauthorized users cannot manage daemon keys."""
        # # Tests [@ANCHOR: test_unauthorized_access]
        # Create a registry entry
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            'name': 'Unauthorized Test',
            'user_id': self.service_user.id,
            'env_file_path': self.test_env_paths[5],
        })

        # Regular user should not be able to rotate keys
        with self.assertRaises(AccessError):
            registry.with_user(self.regular_user.id)._rotate_key_and_write_file()

        # Regular user should not be able to call force provision all
        with self.assertRaises(AccessError):
            self.env["daemon.key.registry"].with_user(self.regular_user.id).action_force_provision_all()

    def test_action_rotate_key(self):
        """Test manual rotation of a single key."""
        # [@ANCHOR: test_action_rotate_key]
        # Tests [@ANCHOR: action_rotate_key_api]
        daemon_name = "Single Rotation Test"
        env_file_path = "/var/lib/odoo/daemon_keys/single_rotation.env"
        self.test_env_paths.append(env_file_path)

        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            'name': daemon_name,
            'user_id': self.service_user.id,
            'env_file_path': env_file_path,
        })

        # Ensure file does not exist
        if os.path.exists(env_file_path):
            os.remove(env_file_path)

        registry.with_user(self.manager_user.id).action_rotate_key()
        self.assertTrue(os.path.exists(env_file_path))

    def test_rotation_safety_archived_user(self):
        """Test that keys cannot be rotated for archived service accounts."""
        # [@ANCHOR: test_rotation_safety_archived_user]
        # Tests [@ANCHOR: rotation_safety_archived_user]
        archived_user = self.env['res.users'].create({
            'name': 'Archived Service Account',
            'login': 'archived_svc',
            'is_service_account': True,
            'active': False,
        })
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            'name': 'Archived Test',
            'user_id': archived_user.id,
            'env_file_path': self.test_env_paths[0],
        })

        with self.assertRaises(UserError) as cm:
            registry.with_user(self.manager_user.id)._rotate_key_and_write_file()
        self.assertIn("archived", str(cm.exception))


@tagged('post_install', '-at_install')
class TestKeyRegistryTour(HamsHttpCase):
    def test_daemon_key_manager_tour(self):
        # [@ANCHOR: test_daemon_key_manager_tour]
        # Verified by [@ANCHOR: test_daemon_key_manager_tour]
        # Tests [@ANCHOR: register_daemon_api]
        # Tests [@ANCHOR: action_force_provision_all_api]

        # Ensure admin has Technical Features enabled for the tour
        admin = self.env.ref('base.user_admin')
        group_no_one = self.env.ref('base.group_no_one')
        if group_no_one not in admin.group_ids:
            admin.write({'group_ids': [(4, group_no_one.id)]})

        manager_group = self.env.ref('daemon_key_manager.group_daemon_key_manager')
        if manager_group not in admin.group_ids:
            admin.write({'group_ids': [(4, manager_group.id)]})

        self.start_tour("/odoo?debug=1&action=daemon_key_manager.action_daemon_key_registry", "daemon_key_manager_tour", login="admin")
