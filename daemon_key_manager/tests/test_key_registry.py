# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import logging
import os
import shutil
from unittest.mock import patch
from odoo.tests import tagged
from odoo.tools import mute_logger
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
            "/opt/hams/etc/keys/exception_test.env",
            "/opt/hams/etc/keys/batch_revoke_a.env",
            "/opt/hams/etc/keys/batch_revoke_b.env",
        ]

        # Directories this suite creates inside the REAL production key
        # directory, which `_cleanup_test_files` cannot remove because
        # `os.remove` does not take directories. They were previously
        # cleaned only by an inline call placed AFTER the assertion that
        # each test exists to make -- so a failing assertion left them
        # behind permanently, inside the directory that holds every
        # daemon's real credentials. `parent_test` is the worst of them:
        # it is deliberately chmod 000, so what a failure leaves behind
        # is an unreadable directory in the credential store.
        self.test_env_dirs = [
            "/opt/hams/etc/keys/trusted",
            "/opt/hams/etc/keys/parent_test",
            "/opt/hams/etc/keys/test_os_error_dir",
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
        self.env["daemon.key.registry"].with_user(self.manager_user.id).search([]).unlink()
        self.service_user.unlink()
        self.regular_user.unlink()
        super().tearDown()

    def _cleanup_test_files(self):
        for path in self.test_env_paths:
            try:
                # `lexists`, not `exists`: `test_security_constraints`
                # plants a symlink to prove the model refuses to follow
                # one out of the allowed prefix, and `exists` answers for
                # the TARGET. A symlink whose target has gone reads as
                # absent and would be left in the credential directory.
                if os.path.lexists(path):
                    os.remove(path)
            except OSError as e:
                _logger.warning("Cleanup error for %s: %s", path, e)
        for directory in self.test_env_dirs:
            try:
                if os.path.lexists(directory):
                    # `parent_test` is deliberately left chmod 000 by the
                    # test that creates it, so rmtree cannot descend into
                    # it until it is readable again.
                    os.chmod(directory, 0o700)
                    shutil.rmtree(directory)
            except OSError as e:
                _logger.warning("Cleanup error for %s: %s", directory, e)

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
        # Tracked in `self.test_env_dirs` (see setUp), so the directory
        # and the symlink inside it are removed even if an assertion
        # below fails. Before that, a failure here left a dangling
        # `evil.env -> /etc/passwd` symlink in the real key directory
        # permanently.
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

    def test_check_user_is_service_account_as_a_narrow_service_account(self):
        """`_check_user_is_service_account` reads `record.user_id.is_service_account` directly
        through the ORM, and that field is `groups="base.group_system"` (zero_sudo/models/
        res_users.py). Reading a group-restricted field through the ORM on a record OTHER than
        `self.env.user` normally raises AccessError for a caller without that group -- confirmed
        directly (a throwaway probe against `browse()`/`.filtered()` on a non-`env.user` record,
        2026-09-16) and the reason the already-fixed zero_sudo res.users.write() password branch
        needed a raw-SQL read instead. `manager_user` (daemon_key_manager.
        user_daemon_key_manager_service, the account register_daemon() actually elevates to) is
        exactly such a narrow, non-system service account.

        Verified here, also directly, that this specific case does NOT hit that AccessError:
        `@api.constrains` validation runs without enforcing the field's own group restriction, so
        `record.user_id.is_service_account` inside a constrains method is safe even though the
        identical read as plain runtime code (e.g. inside write()/an action method) would not be.
        Both directions are asserted so a future Odoo upgrade that changes this constrains
        behavior is caught either way: a real service account target must still be accepted, and
        a non-service-account target must still be refused with the real UserError, not an
        AccessError or a silent pass."""
        record = self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
            {
                "name": "Narrow Caller Accepts Real Service Account",
                "user_id": self.service_user.id,
                "env_file_path": self.test_env_paths[0],
            }
        )
        self.assertTrue(record.exists())

        with self.assertRaises(UserError) as cm:
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
                {
                    "name": "Narrow Caller Rejects Non Service Account",
                    "user_id": self.regular_user.id,
                    "env_file_path": self.test_env_paths[1],
                }
            )
        self.assertNotIsInstance(cm.exception, AccessError)
        self.assertIn("must be a service account", str(cm.exception))

    def test_env_file_path_rejects_literal_directory_traversal(self):
        """_check_env_file_path's ".." check runs on the raw path, before
        os.path.normpath/os.path.realpath -- a distinct, earlier branch
        from the forbidden-prefix and symlink-attack cases test_security_
        constraints already covers, and previously untested on its own.
        """
        with self.assertRaises(UserError) as cm:
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
                {
                    "name": "Traversal Attack",
                    "user_id": self.service_user.id,
                    "env_file_path": "/opt/hams/etc/keys/../../../etc/passwd",
                }
            )
            self.env.flush_all()
        self.assertIn("Directory traversal", str(cm.exception))

        # Even a ".." that would normalize back to a path safely inside the
        # allowed prefix is still rejected -- the check fires on the literal
        # raw string, not the resolved one, confirmed by the distinct error
        # message it raises (proving this branch, not the prefix-mismatch
        # branch, is what caught it).
        with self.assertRaises(UserError) as cm:
            self.env["daemon.key.registry"].with_user(self.manager_user.id).create(
                {
                    "name": "Traversal Within Prefix",
                    "user_id": self.service_user.id,
                    "env_file_path": "/opt/hams/etc/keys/subdir/../test_daemon.env",
                }
            )
            self.env.flush_all()
        self.assertIn("Directory traversal", str(cm.exception))

    def _silent_remove(self, path):
        try:
            if os.path.exists(path):
                os.remove(path)
        except OSError as e:
            _logger.warning("Cleanup error: %s", e)

    def _silent_rmdir(self, path):
        pass # import shutil
        try:
            if os.path.exists(path):
                shutil.rmtree(path)
        except OSError as e:
            _logger.warning("Cleanup error: %s", e)

    def test_register_daemon_api(self):
        """Test the register_daemon API."""
        # [@ANCHOR: COMM_test_register_daemon_api]

        # Tests [@ANCHOR: COMM_register_daemon_api]

        # Tests [@ANCHOR: COMM_register_daemon_logic]

        # Tests [@ANCHOR: COMM_register_daemon_idempotency]

        # Tests [@ANCHOR: COMM_write_secure_env_file_logic]

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

    def test_rotating_again_revokes_the_previous_key(self):
        # Tests [@ANCHOR: COMM_revoke_old_keys_logic]

        # Tests [@ANCHOR: COMM_manager_apikeys_of_service_accounts_rule]
        # The Manager group implies base.group_user, whose own-keys-only record rule
        # used to hide every service account key from the rotation's revoke search:
        # each rotation added a key and none was ever revoked.
        service_user = self.env["res.users"].create(
            {
                "name": "Test Revocation Service Account",
                "login": "test_revocation_svc",
                "is_service_account": True,
            }
        )
        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": "Revocation Test Daemon",
                    "user_id": service_user.id,
                    "env_file_path": self.test_env_paths[2],
                }
            )
        )
        registry._rotate_key_and_write_file()
        self.env.cr.execute(
            "SELECT id FROM res_users_apikeys WHERE name = 'Revocation Test Daemon_key'"
        )
        first_ids = [row[0] for row in self.env.cr.fetchall()]
        self.assertEqual(len(first_ids), 1)

        registry._rotate_key_and_write_file()
        self.env.cr.execute(
            "SELECT id FROM res_users_apikeys WHERE name = 'Revocation Test Daemon_key'"
        )
        second_ids = [row[0] for row in self.env.cr.fetchall()]
        self.assertEqual(len(second_ids), 1, "the previous key was not revoked")
        self.assertNotEqual(second_ids, first_ids)

    def test_batch_rotation_survives_keys_revoked_earlier_in_the_batch(self):
        # Tests [@ANCHOR: COMM_revoke_old_keys_logic]
        # The batch callers (force provision, the rotation cron) fetch every registry's keys
        # once. Rotating the first registry unlinks its old key; reading that deleted record
        # while filtering for the second registry raised MissingError and failed every
        # registry after the first (hams_prod, 2026-10-03).
        registries = []
        for suffix, path in (("A", self.test_env_paths[7]), ("B", self.test_env_paths[8])):
            svc = self.env["res.users"].create(
                {
                    "name": f"Batch Revoke Service {suffix}",
                    "login": f"test_batch_revoke_svc_{suffix.lower()}",
                    "is_service_account": True,
                }
            )
            reg = (
                self.env["daemon.key.registry"]
                .with_user(self.manager_user.id)
                .create({"name": f"Batch Revoke Daemon {suffix}", "user_id": svc.id, "env_file_path": path})
            )
            reg._rotate_key_and_write_file()
            registries.append(reg)

        prefetched = (
            self.env["res.users.apikeys"]
            .with_user(self.manager_user.id)
            .search(
                [
                    ("user_id", "in", [r.user_id.id for r in registries]),
                    ("name", "in", [f"{r.name}_key" for r in registries]),
                ]
            )
        )
        self.assertEqual(len(prefetched), 2)
        for reg in registries:
            reg._rotate_key_and_write_file(pre_fetched_keys=prefetched)

        for reg in registries:
            self.env.cr.execute("SELECT count(*) FROM res_users_apikeys WHERE name = %s", (f"{reg.name}_key",))
            self.assertEqual(self.env.cr.fetchone()[0], 1, f"{reg.name}: old key not revoked or rotation failed")

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

    def test_a_failing_daemon_does_not_undo_or_hide_the_daemons_that_succeeded(self):
        # [@ANCHOR: COMM_test_force_provisioning_partial_failure]

        # Tests [@ANCHOR: COMM_force_provision_error_handling]
        # Found live on hams1, 2026-09-23: one daemon's failure raised UserError at the end, which rolled back the whole
        # transaction, including the API keys of the daemons that HAD succeeded -- whose key files were already
        # written. Files and database then disagreed and those daemons were refused for days.
        good_path = "/opt/hams/etc/keys/partial_ok.env"
        bad_path = "/opt/hams/etc/keys/partial_bad.env"
        self.test_env_paths.extend([good_path, bad_path])
        for path in (good_path, bad_path):
            if os.path.exists(path):
                os.remove(path)
        registry_model = self.env["daemon.key.registry"].with_user(self.manager_user.id)
        registry_model.create(
            {"name": "Partial OK", "user_id": self.service_user.id, "env_file_path": good_path}
        )
        other = self.env["res.users"].create(
            {"name": "Partial Bad Service", "login": "partial_bad_svc", "is_service_account": True}
        )
        registry_model.create({"name": "Partial Bad", "user_id": other.id, "env_file_path": bad_path})
        real_rotate = type(registry_model)._rotate_key_and_write_file

        def _rotate(reg, *args, **kwargs):
            if reg.name == "Partial Bad":
                raise OSError("simulated unwritable key file")
            return real_rotate(reg, *args, **kwargs)

        with patch.object(type(registry_model), "_rotate_key_and_write_file", _rotate):
            result = registry_model.action_force_provision_all()  # must not raise

        self.assertEqual(result["params"]["type"], "danger")
        self.assertIn("Partial Bad", result["params"]["message"])
        self.assertNotIn("Partial OK", result["params"]["message"].split("FAILED for:")[1])
        self.assertTrue(os.path.exists(good_path), "the daemon that could be provisioned still is")
        self.assertFalse(os.path.exists(bad_path))
        self.assertTrue(
            self.env["res.users.apikeys"].search_count(
                [("user_id", "=", self.service_user.id), ("name", "=", "Partial OK_key")]
            ),
            "the successful daemon's key is in the database, in step with its file",
        )

    def test_force_provision_skips_a_removed_daemons_archived_account(self):
        # [@ANCHOR: COMM_test_force_provision_skips_archived]
        # Tests [@ANCHOR: COMM_force_provision_skips_archived]
        # hams1, 2026-10-05: the registry row of the removed wildcard-certificate renewal (archived service account)
        # made hams.daemon.keys.service exit 1 after every run. It is skipped, not a failure.
        good_path = "/opt/hams/etc/keys/skip_ok.env"
        old_path = "/opt/hams/etc/keys/skip_old.env"
        self.test_env_paths.extend([good_path, old_path])
        for path in (good_path, old_path):
            if os.path.exists(path):
                os.remove(path)
        registry_model = self.env["daemon.key.registry"].with_user(self.manager_user.id)
        registry_model.create({"name": "Skip OK", "user_id": self.service_user.id, "env_file_path": good_path})
        archived = self.env["res.users"].create(
            {"name": "Removed Daemon", "login": "removed_daemon_svc", "is_service_account": True, "active": False}
        )
        registry_model.create({"name": "Skip Old", "user_id": archived.id, "env_file_path": old_path})

        with self.assertLogs("odoo.addons.daemon_key_manager.models.key_registry", level="INFO") as logs:
            result = registry_model.action_force_provision_all()

        # Other registry rows on the test database may or may not provision here; this removed daemon is not
        # among the failures, and it was skipped with one INFO line.
        self.assertNotIn("Skip Old", (result["params"]["message"].split("FAILED for:") + [""])[1])
        self.assertTrue(any("Skipping key provisioning" in line and "Skip Old" in line for line in logs.output))
        self.assertTrue(os.path.exists(good_path))
        self.assertFalse(os.path.exists(old_path))

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

    def test_action_rotate_key_self_heals_a_service_account_missing_the_usage_group(self):
        """
        Real bug found 2026-09-28/29 (a live production incident, `backup_worker` unable to
        rotate its own key): `group_daemon_key_usage` (the group that grants a 90-day API-key
        duration) was only ever granted dynamically, by `register_daemon()`, via a raw-SQL
        insert -- never declared statically in any daemon service account's own `(6, 0, [...])`
        `group_ids` eval. Since every one of those XML records lives in a `noupdate="0"` block
        specifically so an ordinary module upgrade can *replace* the account's group set (see
        `backup_management/security/security.xml`'s own 2026-09-14 comment), the very next
        upgrade after a daemon's first registration silently wiped the dynamic grant back out,
        with no error anywhere -- confirmed live: `action_rotate_key()` then fails with
        `ValidationError: You cannot exceed 1.0 days` the next time anything tries to rotate
        that daemon's key, since a service account holding none of this module's own groups
        gets the ordinary 1-day default duration ceiling.

        This test reproduces the account-missing-the-group half directly (the module-upgrade
        wipe itself isn't reproduced here -- see this test's own docstring for why that part is
        already covered by every `is_service_account` XML record's own review, not a unit test)
        and asserts `action_rotate_key()` now self-heals: it grants the group itself
        (`_ensure_usage_group()`, called from `_rotate_key_and_write_file()`) before generating
        the key, rather than assuming some earlier caller already guaranteed it.
        """
        usage_group = self.env.ref("daemon_key_manager.group_daemon_key_usage")
        self.assertNotIn(
            usage_group,
            self.service_user.group_ids,
            "This test's own premise requires the service account to start without the usage "
            "group -- if it already has it, this test is no longer exercising the real gap.",
        )

        daemon_name = "Self Heal Rotation Test"
        env_file_path = "/opt/hams/etc/keys/self_heal_rotation.env"
        self.test_env_paths.append(env_file_path)
        if os.path.exists(env_file_path):
            os.remove(env_file_path)

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

        # Before the fix, this raised ValidationError: You cannot exceed 1.0 days.
        registry.with_user(self.manager_user.id).action_rotate_key()

        self.assertTrue(os.path.exists(env_file_path))
        self.service_user.invalidate_recordset()
        self.assertIn(
            usage_group,
            self.service_user.group_ids,
            "action_rotate_key() should grant the usage group itself when it's missing, not "
            "just succeed by luck.",
        )

    def test_rotated_key_bearer_json2_not_blocked_for_service_account(self):
        """A daemon calls /json/2/... with the Bearer key this module
        provisions, as a service account. zero_sudo's ir.http._authenticate
        blocks any request whose request.session.uid is a service account
        (except /jsonrpc and /xmlrpc/); Odoo's auth='bearer' only calls
        request.update_env(user=uid) and never sets session.uid, so daemons
        must not be blocked. Real key, real env file, real HTTP request."""
        # Tests [@ANCHOR: zero_sudo:ir_http_authenticate]
        env_file_path = "/opt/hams/etc/keys/bearer_json2.env"
        self.test_env_paths.append(env_file_path)
        registry = (
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .create(
                {
                    "name": "Bearer Json2 Test",
                    "user_id": self.service_user.id,
                    "env_file_path": env_file_path,
                }
            )
        )
        registry.with_user(self.manager_user.id).action_rotate_key()
        key = None
        with open(env_file_path, "r") as f:  # audit-ignore-path
            for line in f:
                if line.startswith("ODOO_RPC_KEY="):
                    key = line.strip().split("=", 1)[1]
        self.assertTrue(key, "The provisioned env file must carry the key.")
        self.assertTrue(
            self.env["ir.http"]._is_service_account_cached(self.service_user.id),
            "Precondition: the user must really be a service account.",
        )
        # RealTransactionCase: the HTTP worker needs the committed key.
        self.env.cr.commit()

        response = self.url_open(
            "/json/2/res.users/context_get",
            data="{}",
            headers={
                "Authorization": f"Bearer {key}",
                "Content-Type": "application/json",
                "X-Odoo-Database": self.env.cr.dbname,
            },
            allow_redirects=False,
        )
        self.assertEqual(
            response.status_code,
            200,
            msg=f"Bearer service account must not be blocked: {response.text[:300]}",
        )
        self.assertNotIn("Interactive Web UI access is denied", response.text)

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

        # Trigger PermissionError naturally by trying to write to a child of a 000 dir
        parent_dir = "/opt/hams/etc/keys/parent_test"
        if not os.path.exists(parent_dir):
            os.makedirs(parent_dir)
            os.chmod(parent_dir, 0o000)
            
        with self.assertRaises(PermissionError):
            registry._write_secure_env_file(f"{parent_dir}/child/test.env", "login", "key")
            
        # `parent_dir` and `test_os_error_dir` are both tracked in
        # `self.test_env_dirs` (see setUp) and removed by tearDown on
        # every path, including a failing one. They used to be cleaned
        # inline, after the assertion each one exists to make.
        os.makedirs("/opt/hams/etc/keys/test_os_error_dir", exist_ok=True)
        with self.assertRaises(OSError):
            registry._write_secure_env_file("/opt/hams/etc/keys/test_os_error_dir", "login", "key")

    def test_write_secure_env_file_refuses_to_write_a_credential_when_fchmod_fails(self):
        """Real, CRITICAL fix, found by an adversarial security review,
        live-reproduced on the real dev box: the old code opened the
        REAL target path directly with O_CREAT|O_TRUNC. O_CREAT is a
        no-op when the target already exists (e.g. left behind by an
        earlier run under a different OS user/ownership) -- open() could
        still succeed as long as the EXISTING file's own permissions
        happened to allow this process to write it, with only the later
        fchmod() failing (this process isn't the file's owner). That
        failure used to just be logged as a warning while a fresh,
        currently-valid credential got written into the still-
        insecurely-permissioned file anyway -- confirmed live: every
        ~59-day rotation cycle re-armed a real exposure this way. Worse,
        O_TRUNC destroyed the target's prior content immediately on open,
        before any permission problem could even be detected.

        Real fix: write to a brand-new temp file (always correctly owned
        and 0600 from creation) and atomically rename it onto the real
        target -- the real target's own permissions/ownership never
        matter at all, and its content is never touched unless the whole
        write genuinely succeeds. This test simulates a write-time
        failure (mocking `os.fchmod` on the temp file, since a real
        failure there in production would mean something more seriously
        wrong, e.g. disk full or a filesystem-level problem) and confirms
        the target file's own prior content is left completely
        untouched, and no stray temp file is left behind either.
        """
        registry = self.env["daemon.key.registry"].with_user(self.manager_user.id).create({
            "name": "Fchmod Failure Test Daemon",
            "user_id": self.service_user.id,
            "env_file_path": "/opt/hams/etc/keys/fchmod_failure_test.env",
        })
        path = "/opt/hams/etc/keys/fchmod_failure_test.env"
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:  # audit-ignore-path
            f.write("PRE-EXISTING-CONTENT-FROM-A-DIFFERENT-OWNER\n")
        self.addCleanup(lambda: os.path.exists(path) and os.remove(path))

        self.safe_patch("os.fchmod", side_effect=PermissionError("simulated: cannot secure the fresh temp file"))
        with self.assertRaises(PermissionError):
            registry._write_secure_env_file(path, "login", "key")

        with open(path, "r") as f:  # audit-ignore-path
            content = f.read()
        self.assertEqual(
            content,
            "PRE-EXISTING-CONTENT-FROM-A-DIFFERENT-OWNER\n",
            "a failed write must never overwrite the file's prior content with a live credential.",
        )
        leftover_temp_files = [
            name for name in os.listdir(os.path.dirname(path)) if name.startswith(".daemon_key_")
        ]
        self.assertEqual(leftover_temp_files, [], "a failed write must not leave a stray temp file behind.")

    def test_register_daemon_rejects_a_colliding_env_file_path(self):
        # Adversarial security review, 2026-09-03: register_daemon()'s own
        # authorization check only verifies the caller IS the target
        # service account -- it never verified env_file_path was unique.
        # A caller could previously register a brand-new daemon_name
        # while pointing env_file_path at a DIFFERENT, already-registered
        # daemon's real credential file, and register_daemon() would
        # happily create the new row and overwrite that other daemon's
        # real .env with its own (lower-privileged) credentials on the
        # very next rotation. [!] DIAGNOSTIC FOR AI: this must now fail
        # closed, before any file I/O happens.
        real_path = "/opt/hams/etc/keys/collision_victim.env"
        self.test_env_paths.append(real_path)
        self.env["daemon.key.registry"].with_user(self.manager_user.id).register_daemon(
            "Collision Victim Daemon",
            "daemon_key_manager.user_daemon_key_manager_service",
            real_path,
        )
        self.assertTrue(
            os.path.exists(real_path), "test setup assumption: the victim's real .env was written"
        )
        with open(real_path, "r") as f:  # audit-ignore-path
            original_content = f.read()

        with mute_logger("odoo.sql_db"):
            with self.assertRaises(Exception):
                self.env["daemon.key.registry"].with_user(self.manager_user.id).register_daemon(
                    "Attacker Daemon",
                    "daemon_key_manager.user_daemon_key_manager_service",
                    real_path,
                )

        with open(real_path, "r") as f:  # audit-ignore-path
            content_after = f.read()
        self.assertEqual(
            content_after,
            original_content,
            "[!] DIAGNOSTIC FOR AI: a rejected registration must not "
            "overwrite the real victim daemon's credential file at all.",
        )
        self.assertEqual(
            self.env["daemon.key.registry"]
            .with_user(self.manager_user.id)
            .search_count([("name", "=", "Attacker Daemon")]),
            0,
            "the colliding registration must not have been persisted either.",
        )


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
