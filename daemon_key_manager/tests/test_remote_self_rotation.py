# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Remote self-rotation (rotate_own_key) for a daemon on another machine.

The real case: a sync daemon that runs on a separate machine, not on the Odoo
host, so the Odoo-side rotation (revoke the old key, write the new one to a local file)
would strand it on a revoked key. These tests drive rotate_own_key() the way
that daemon does: real JSON-2 HTTP requests carrying a real Bearer key, against
committed data (RealTransactionCase), and check which keys Odoo then accepts.
"""
import datetime
import json
import logging
import os

from odoo import fields
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import UserError, ValidationError


_logger = logging.getLogger(__name__)

ROTATE_URL = "/json/2/daemon.key.registry/rotate_own_key"
WHOAMI_URL = "/json/2/res.users/context_get"


@tagged("post_install", "-at_install")
class TestRemoteSelfRotation(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.env_paths = [
            "/opt/hams/etc/keys/remote_rotation_test.env",
            "/opt/hams/etc/keys/remote_rotation_other.env",
        ]
        self._remove_env_files()
        self.manager_user = self.env.ref(
            "daemon_key_manager.user_daemon_key_manager_service"
        )
        self.service_user = self.env["res.users"].create(
            {
                "name": "Remote Rotation Service Account",
                "login": "remote_rotation_svc",
                "is_service_account": True,
            }
        )
        self.registry_model = self.env["daemon.key.registry"].with_user(
            self.manager_user.id
        )

    def tearDown(self):
        # A test's HTTP calls commit on their own cursors after this cursor's
        # snapshot may already be open; deleting a row they updated would then
        # fail with a serialization error. Start a fresh transaction first.
        self._reload()
        self._remove_env_files()
        self.registry_model.search(
            [("user_id", "=", self.service_user.id)]
        ).unlink()
        # The account's keys go with it (res_users_apikeys.user_id is ON DELETE CASCADE).
        self.service_user.unlink()
        self.env.cr.commit()
        super().tearDown()

    def _reload(self):
        """
        Ends this cursor's transaction and drops the ORM cache, so the next read
        sees what the HTTP requests (committed on their own cursors) wrote. The
        cursor is REPEATABLE READ: without this, a read after an HTTP call can
        return the snapshot taken before it. Nothing is pending on this cursor at
        the call sites (every write here is followed by a commit).
        """
        self.env.cr.commit()
        self.env.invalidate_all()

    def _remove_env_files(self):
        for path in self.env_paths:
            if os.path.lexists(path):
                os.remove(path)

    def _read_key(self, path):
        with open(path, "r") as env_file:  # audit-ignore-path
            for line in env_file:
                if line.startswith("ODOO_RPC_KEY="):
                    return line.strip().split("=", 1)[1]
        return None

    def _remote_registry(self, name, path):
        """A registry provisioned the ordinary way, then switched to remote."""
        registry = self.registry_model.create(
            {"name": name, "user_id": self.service_user.id, "env_file_path": path}
        )
        registry.action_rotate_key()
        registry.write({"remote_self_rotation": True})
        self.env.cr.commit()
        return registry, self._read_key(path)

    def _make_due(self, registry):
        long_ago = fields.Datetime.now() - datetime.timedelta(days=60)
        registry.write({"last_rotated": long_ago})
        self.env.cr.commit()

    def _post(self, url, key, body):
        return self.url_open(
            url,
            data=json.dumps(body),
            headers={
                "Authorization": f"Bearer {key}",
                "Content-Type": "application/json",
                "X-Odoo-Database": self.env.cr.dbname,
            },
            allow_redirects=False,
        )

    def _rotate(self, name, key):
        return self._post(ROTATE_URL, key, {"daemon_name": name, "current_key": key})

    def _key_accepted(self, key):
        return self._post(WHOAMI_URL, key, {}).status_code == 200

    def test_two_phase_rotation_revokes_nothing_until_the_new_key_is_proven(self):
        # [@ANCHOR: COMM_test_rotate_own_key_two_phase]

        # Tests [@ANCHOR: COMM_rotate_own_key_api]

        # Tests [@ANCHOR: COMM_rotate_own_key_issue]

        # Tests [@ANCHOR: COMM_rotate_own_key_confirm]
        name = "Remote Rotation Test"
        path = self.env_paths[0]
        registry, old_key = self._remote_registry(name, path)
        self.assertTrue(old_key)

        # Freshly rotated: not due, nothing changes.
        response = self._rotate(name, old_key)
        self.assertEqual(response.status_code, 200, response.text[:300])
        self.assertEqual(response.json()["status"], "not_due")

        self._make_due(registry)
        response = self._rotate(name, old_key)
        self.assertEqual(response.status_code, 200, response.text[:300])
        issued = response.json()
        self.assertEqual(issued["status"], "issued")
        self.assertEqual(issued["login"], self.service_user.login)
        new_key = issued["key"]
        self.assertNotEqual(new_key, old_key)
        self.assertTrue(self._key_accepted(old_key), "old key must survive step 1")
        self.assertTrue(self._key_accepted(new_key))

        response = self._rotate(name, new_key)
        self.assertEqual(response.status_code, 200, response.text[:300])
        self.assertEqual(response.json()["status"], "confirmed")
        self.assertFalse(self._key_accepted(old_key), "old key revoked at step 2")
        self.assertTrue(self._key_accepted(new_key))

        self._reload()
        self.assertEqual(registry.pending_key_id, 0)
        age = fields.Datetime.now() - registry.last_rotated
        self.assertLess(age, datetime.timedelta(minutes=5))
        self.assertEqual(self._read_key(path), new_key, "local copy kept current")

        # And the cycle restarts: the new key is now the active one, not due.
        response = self._rotate(name, new_key)
        self.assertEqual(response.json()["status"], "not_due")

    def test_a_lost_step_one_reply_is_replaced_not_accumulated(self):
        # Tests [@ANCHOR: COMM_rotate_own_key_issue]
        name = "Remote Rotation Test"
        registry, old_key = self._remote_registry(name, self.env_paths[0])
        self._make_due(registry)

        lost_key = self._rotate(name, old_key).json()["key"]
        retried = self._rotate(name, old_key).json()
        self.assertEqual(retried["status"], "issued")

        self.assertFalse(self._key_accepted(lost_key), "unconfirmed key replaced")
        self.assertTrue(self._key_accepted(old_key))
        self.assertTrue(self._key_accepted(retried["key"]))
        self.assertEqual(
            self._rotate(name, retried["key"]).json()["status"], "confirmed"
        )

    def test_refused_for_a_registry_that_is_not_remote(self):
        # Tests [@ANCHOR: COMM_rotate_own_key_api]
        name = "Local Rotation Test"
        path = self.env_paths[0]
        registry = self.registry_model.create(
            {"name": name, "user_id": self.service_user.id, "env_file_path": path}
        )
        registry.action_rotate_key()
        self._make_due(registry)
        key = self._read_key(path)

        response = self._rotate(name, key)
        self.assertNotEqual(response.status_code, 200)
        self.assertIn("AccessError", response.text)
        self.assertTrue(self._key_accepted(key))

    def test_refused_when_the_key_belongs_to_another_registry_of_the_account(self):
        # Tests [@ANCHOR: COMM_rotate_own_key_api]
        name = "Remote Rotation Test"
        registry, remote_key = self._remote_registry(name, self.env_paths[0])
        self._make_due(registry)
        other = self.registry_model.create(
            {
                "name": "Same Account Other Daemon",
                "user_id": self.service_user.id,
                "env_file_path": self.env_paths[1],
            }
        )
        other.action_rotate_key()
        self.env.cr.commit()
        other_key = self._read_key(self.env_paths[1])

        response = self._rotate(name, other_key)
        self.assertNotEqual(response.status_code, 200)
        self.assertIn("AccessError", response.text)
        self._reload()
        self.assertEqual(registry.pending_key_id, 0)
        self.assertTrue(self._key_accepted(remote_key))

    def test_odoo_side_rotation_paths_leave_a_remote_registry_alone(self):
        # Tests [@ANCHOR: COMM_remote_self_rotation_excluded_from_local_rotation]

        # Tests [@ANCHOR: COMM_cron_rotation_logic]

        # Tests [@ANCHOR: COMM_force_provision_logic]
        name = "Remote Rotation Test"
        registry, remote_key = self._remote_registry(name, self.env_paths[0])
        self._make_due(registry)

        self.registry_model._cron_rotate_all_keys()
        self.registry_model.action_force_provision_all()
        with self.assertRaises(UserError):
            registry.action_rotate_key()
        self.env.cr.commit()

        self.assertTrue(
            self._key_accepted(remote_key),
            "cron, force-provision and Rotate Key must not revoke a remote key",
        )

    def _active_key_expiries(self, registry):
        """Expiry of every key the registry's account holds under its key name, soonest first."""
        self._reload()
        self.env.cr.execute(
            "SELECT expiration_date FROM res_users_apikeys WHERE user_id = %s AND name = %s"
            " ORDER BY expiration_date",
            (registry.user_id.id, f"{registry.name}_key"),
        )
        return [row[0] for row in self.env.cr.fetchall()]

    def _days_from_now(self, moment):
        return (moment - datetime.datetime.utcnow()).total_seconds() / 86400

    def test_key_lifetime_is_only_for_a_remote_row_and_within_range(self):
        # Tests [@ANCHOR: COMM_action_apply_key_lifetime]
        path = self.env_paths[0]
        local = self.registry_model.create(
            {"name": "Local Lifetime Test", "user_id": self.service_user.id, "env_file_path": path}
        )
        with self.assertRaises(ValidationError):
            local.write({"key_lifetime_days": 365})
        self.env.cr.rollback()
        remote, _key = self._remote_registry("Remote Lifetime Test", self.env_paths[1])
        for bad in (89, 401, -5):
            with self.assertRaises(ValidationError):
                remote.write({"key_lifetime_days": bad})
            self.env.cr.rollback()
        remote.write({"key_lifetime_days": 365})
        self.env.cr.commit()
        self.assertEqual(remote._lifetime_days(), 365)
        self.assertEqual(remote._rotation_age_days(), 334)
        remote.write({"key_lifetime_days": 0})
        self.assertEqual(remote._lifetime_days(), 90)
        self.assertEqual(remote._rotation_age_days(), 59)

    def test_a_year_long_lifetime_extends_the_held_key_and_mints_year_long_keys(self):
        # Tests [@ANCHOR: COMM_action_apply_key_lifetime]
        name = "Remote Lifetime Year Test"
        registry, old_key = self._remote_registry(name, self.env_paths[0])
        (before,) = self._active_key_expiries(registry)
        self.assertAlmostEqual(self._days_from_now(before), 90, delta=1)

        # A default row: applying changes nothing.
        self.assertEqual(registry.action_apply_key_lifetime(), 0)
        self.env.cr.commit()
        (unchanged,) = self._active_key_expiries(registry)
        self.assertEqual(unchanged, before)

        registry.write({"key_lifetime_days": 365})
        self.env.cr.commit()
        self.assertEqual(registry.action_apply_key_lifetime(), 1)
        self.env.cr.commit()
        (extended,) = self._active_key_expiries(registry)
        self.assertAlmostEqual(self._days_from_now(extended), 365, delta=1)
        self.assertTrue(self._key_accepted(old_key), "the daemon's key string is unchanged")
        long_group = self.env.ref("daemon_key_manager.group_daemon_key_usage_long")
        self.assertIn(long_group, self.service_user.group_ids)
        # Applying again never shortens: still about a year.
        registry.action_apply_key_lifetime()
        self.env.cr.commit()
        (again,) = self._active_key_expiries(registry)
        self.assertGreaterEqual(again, extended)

        # 60 days on, a 90-day key would be due; a year-long one is not.
        self._make_due(registry)
        self.assertEqual(self._rotate(name, old_key).json()["status"], "not_due")
        # It is due 334 days after the last rotation, and the new key is a year long too.
        registry.write({"last_rotated": fields.Datetime.now() - datetime.timedelta(days=335)})
        self.env.cr.commit()
        issued = self._rotate(name, old_key).json()
        self.assertEqual(issued["status"], "issued")
        self.assertEqual(self._rotate(name, issued["key"]).json()["status"], "confirmed")
        (renewed,) = self._active_key_expiries(registry)
        self.assertAlmostEqual(self._days_from_now(renewed), 365, delta=1)
