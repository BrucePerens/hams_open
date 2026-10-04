# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""A key file handed to a daemon family's OS group (hamsd_<family>).

These tests write real files in the real key directory and read their real modes and groups
back. The group is the real `hamsd_ncvec_sync` group the provisioning creates (the first
daemon family to run under its own account, docs/proposals/DAEMON_OS_ISOLATION_PLAN.md in
hams_com); a host where provisioning has not run fails the setUp check below with that
instruction rather than skipping, because a skipped test would hide a host that cannot hand a
key to any daemon account.

One-shot fix on a test host that predates this change (run as root from a checkout that has it):
    python3 -c "import subprocess, sys; sys.path.insert(0, 'hams_shared/tools'); import infrastructure as i;
    run = lambda c: subprocess.run(c, check=True);
    i.provision_system_accounts(run, 'test'); i.apply_production_directories(run, 'test')"
The Odoo server must then be started afresh (a running process keeps the groups it started with).
"""
import grp
import os
import shutil
import stat

from odoo.exceptions import AccessError, UserError
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.tests import tagged

FAMILY_GROUP = "hamsd_ncvec_sync"
KEY_ROOT = "/opt/hams/etc/keys"
TEST_DIR = os.path.join(KEY_ROOT, "os_group_test")
ROOT_PROBE = os.path.join(KEY_ROOT, "os_group_root_probe.env")
REGISTERED = os.path.join(TEST_DIR, "registered.env")
MANAGER_XML_ID = "daemon_key_manager.user_daemon_key_manager_service"


@tagged("post_install", "-at_install")
class TestOsGroupKeyFile(RealTransactionCase):
    def setUp(self):
        super().setUp()
        try:
            self.family_gid = grp.getgrnam(FAMILY_GROUP).gr_gid
        except KeyError:  # burn-ignore-os-account-probe
            self.fail(
                f"The OS group {FAMILY_GROUP} does not exist on this host: run the "
                "provisioning (provision.py, or test.py's own) so the daemon account is created."
            )
        self.assertIn(
            self.family_gid,
            os.getgroups(),
            f"This process is not in {FAMILY_GROUP}: provisioning adds the Odoo user to it, "
            "and the server must be started afterwards.",
        )
        self.registry_model = self.env["daemon.key.registry"]
        self.manager_user = self.env.ref(MANAGER_XML_ID)
        self._cleanup()
        self.service_user = self.env["res.users"].create(
            {
                "name": "OS Group Test Service Account",
                "login": "os_group_test_service_account",
                "is_service_account": True,
            }
        )

    def tearDown(self):
        self._cleanup()
        self.registry_model.with_user(self.manager_user.id).search([]).unlink()
        self.service_user.unlink()
        super().tearDown()

    def _cleanup(self):
        if os.path.lexists(ROOT_PROBE):
            os.remove(ROOT_PROBE)
        if os.path.lexists(TEST_DIR):
            os.chmod(TEST_DIR, 0o700)
            shutil.rmtree(TEST_DIR)

    def _write(self, path, key="key-value", group=None):
        self.registry_model._write_secure_env_file(path, "login", key, group=group)

    def _mode(self, path):
        return stat.S_IMODE(os.stat(path).st_mode)

    def test_a_group_file_is_0640_in_that_group_and_owned_by_the_writer(self):
        # Tests [@ANCHOR: COMM_write_secure_env_file_group]
        path = os.path.join(TEST_DIR, "group.env")
        self._write(path, key="the-key", group=FAMILY_GROUP)
        info = os.stat(path)
        self.assertEqual(stat.S_IMODE(info.st_mode), 0o640)
        self.assertEqual(info.st_gid, self.family_gid)
        self.assertEqual(info.st_uid, os.geteuid())
        with open(path, encoding="utf-8") as key_file:
            self.assertIn("ODOO_RPC_KEY=the-key\n", key_file.read())
        self.assertEqual(self._mode(TEST_DIR), 0o700, "a subdirectory stays 0700")

    def test_without_a_group_the_file_stays_0600(self):
        # Tests [@ANCHOR: COMM_write_secure_env_file_logic]
        path = os.path.join(TEST_DIR, "plain.env")
        self._write(path)
        info = os.stat(path)
        self.assertEqual(stat.S_IMODE(info.st_mode), 0o600)
        self.assertEqual(info.st_gid, os.getegid())

    def test_rewriting_a_group_file_keeps_the_group_and_replaces_the_key(self):
        path = os.path.join(TEST_DIR, "rewrite.env")
        self._write(path, key="first", group=FAMILY_GROUP)
        self._write(path, key="second", group=FAMILY_GROUP)
        self.assertEqual(os.stat(path).st_gid, self.family_gid)
        self.assertEqual(self._mode(path), 0o640)
        with open(path, encoding="utf-8") as key_file:
            content = key_file.read()
        self.assertIn("ODOO_RPC_KEY=second\n", content)
        self.assertNotIn("first", content)

    def test_a_missing_group_is_refused_and_the_old_file_is_untouched(self):
        # Tests [@ANCHOR: COMM_resolve_os_group]
        path = os.path.join(TEST_DIR, "missing_group.env")
        self._write(path, key="old", group=FAMILY_GROUP)
        with self.assertRaises(UserError) as caught:
            self._write(path, key="new", group="hamsd_no_such_family_x")
        self.assertIn("does not exist", str(caught.exception))
        with open(path, encoding="utf-8") as key_file:
            self.assertIn("ODOO_RPC_KEY=old\n", key_file.read())
        self.assertEqual(os.listdir(TEST_DIR), ["missing_group.env"], "no temp file left behind")

    def test_a_group_outside_the_daemon_family_namespace_is_refused(self):
        # Tests [@ANCHOR: COMM_resolve_os_group]
        path = os.path.join(TEST_DIR, "wrong_name.env")
        for name in ("hams_com", "adm", "root", "hamsd_", "hamsd_UPPER", "hamsd_a-b"):
            with self.subTest(group=name):
                with self.assertRaises(UserError):
                    self._write(path, group=name)
        self.assertFalse(os.path.exists(path))

    def test_chgrp_refused_by_the_kernel_leaves_the_old_file_and_no_temp_file(self):
        # The server started before provisioning added it to the group: the kernel refuses the
        # chgrp. Simulated with the one failing call, standard mocking for a non-daemon test.
        path = os.path.join(TEST_DIR, "refused.env")
        self._write(path, key="old", group=FAMILY_GROUP)
        target = "odoo.addons.daemon_key_manager.models.key_registry.os.fchown"
        self.safe_patch(target, side_effect=PermissionError(1, "Operation not permitted"))
        with self.assertRaises(UserError) as caught:
            self._write(path, key="new", group=FAMILY_GROUP)
        self.assertIn("not a member of the OS group", str(caught.exception))
        with open(path, encoding="utf-8") as key_file:
            self.assertIn("ODOO_RPC_KEY=old\n", key_file.read())
        self.assertEqual(os.listdir(TEST_DIR), ["refused.env"])

    def test_the_key_root_directory_is_kept_at_0710(self):
        # Tests [@ANCHOR: COMM_key_root_directory_mode]
        os.chmod(KEY_ROOT, 0o700)
        self._write(ROOT_PROBE, group=FAMILY_GROUP)
        self.assertEqual(self._mode(KEY_ROOT), 0o710)

    def test_the_registry_accepts_only_daemon_family_group_names(self):
        # Tests [@ANCHOR: COMM_security_constraints_os_group]
        # [@ANCHOR: COMM_test_os_group_registry_constraint]
        registry = self.registry_model.with_user(self.manager_user.id)
        bad_names = ("hams_com", "adm", "hamsd_", "hamsd_Bad", "hamsd_x y")
        for index, name in enumerate(bad_names):
            with self.subTest(group=name):
                with self.assertRaises(UserError):
                    registry.create(
                        {
                            "name": f"Bad group {name}",
                            "user_id": self.service_user.id,
                            "env_file_path": os.path.join(TEST_DIR, f"bad_{index}.env"),
                            "os_group": name,
                        }
                    )
                    self.env.flush_all()
        good = registry.create(
            {
                "name": "Good group",
                "user_id": self.service_user.id,
                "env_file_path": REGISTERED,
                "os_group": FAMILY_GROUP,
            }
        )
        self.env.flush_all()
        self.assertEqual(good.os_group, FAMILY_GROUP)

    def test_register_daemon_writes_the_group_file_and_keeps_it_across_reregistration(self):
        # Tests [@ANCHOR: COMM_register_daemon_idempotency]
        registry = self.registry_model.with_user(self.manager_user.id)
        registry.register_daemon("OS Group Daemon", MANAGER_XML_ID, REGISTERED, os_group=FAMILY_GROUP)
        self.assertEqual(os.stat(REGISTERED).st_gid, self.family_gid)
        self.assertEqual(self._mode(REGISTERED), 0o640)

        # None means "leave the group alone": the hook that registered the daemon can run again.
        registry.register_daemon("OS Group Daemon", MANAGER_XML_ID, REGISTERED)
        row = registry.search([("name", "=", "OS Group Daemon")])
        row.invalidate_recordset()
        self.assertEqual(row.os_group, FAMILY_GROUP)
        self.assertEqual(self._mode(REGISTERED), 0o640)

        # An empty string clears it, and the next rotation writes a 0600 file again.
        registry.register_daemon("OS Group Daemon", MANAGER_XML_ID, REGISTERED, os_group="")
        row.invalidate_recordset()
        self.assertFalse(row.os_group)
        self.assertEqual(self._mode(REGISTERED), 0o600)
        self.assertEqual(os.stat(REGISTERED).st_gid, os.getegid())

    def test_a_rotation_keeps_the_group(self):
        registry = self.registry_model.with_user(self.manager_user.id)
        registry.register_daemon("OS Group Rotation", MANAGER_XML_ID, REGISTERED, os_group=FAMILY_GROUP)
        with open(REGISTERED, encoding="utf-8") as key_file:
            before = key_file.read()
        registry.search([("name", "=", "OS Group Rotation")]).action_rotate_key()
        with open(REGISTERED, encoding="utf-8") as key_file:
            after = key_file.read()
        self.assertNotEqual(before, after, "the rotation wrote a new key")
        self.assertEqual(os.stat(REGISTERED).st_gid, self.family_gid)
        self.assertEqual(self._mode(REGISTERED), 0o640)

    def test_a_service_account_cannot_set_its_own_key_files_group(self):
        # Tests [@ANCHOR: COMM_register_daemon_os_group_manager_only]
        registry = self.registry_model.with_user(self.service_user.id)
        with self.assertRaises(AccessError):
            registry.register_daemon(
                "Self Widening",
                self.service_user.login,
                REGISTERED,
                os_group=FAMILY_GROUP,
            )
        self.assertFalse(os.path.exists(REGISTERED))
