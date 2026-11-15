# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Refusal branches of the key registry that the other tests did not reach: who may register a daemon, change a key's
lifetime or rotate a key, what a self-rotating daemon must send, which paths a key may be written to, and what the
rotation cron does when a rotation fails in an unexpected way."""
import datetime
import os

from odoo import fields
from odoo.exceptions import AccessError, UserError
from odoo.tests import tagged

from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestKeyRegistryRefusals(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        self.model = self.env["daemon.key.registry"]
        self.service = self.env["res.users"].create(
            {"name": "Refusal Service", "login": "refusal_service", "is_service_account": True}
        )
        self.other_service = self.env["res.users"].create(
            {"name": "Refusal Other Service", "login": "refusal_other_service", "is_service_account": True}
        )
        self.regular = self.env["res.users"].create(
            {"name": "Refusal Regular", "login": "refusal_regular", "is_service_account": False}
        )
        self.path = "/opt/hams/etc/keys/refusal_test.env"
        self.row = self.model.with_user(self.manager.id).create(
            {"name": "Refusal Row", "user_id": self.service.id, "env_file_path": self.path}
        )

    def tearDown(self):
        self.model.with_user(self.manager.id).search([("name", "=", "Refusal Row")]).unlink()
        for path in (self.path, "/opt/hams/etc/keys/refusal_new.env"):
            if os.path.lexists(path):
                os.remove(path)
        (self.service | self.other_service | self.regular).unlink()
        super().tearDown()

    def test_register_daemon_for_an_unknown_login_is_refused(self):
        # Tests [@ANCHOR: COMM_register_daemon_api]
        with self.assertRaisesRegex(UserError, "not found"):
            self.model.with_user(self.manager.id).register_daemon(
                "Refusal New", "no_such_login_anywhere", "/opt/hams/etc/keys/refusal_new.env"
            )

    def test_register_daemon_by_an_ordinary_user_is_refused(self):
        # Tests [@ANCHOR: COMM_register_daemon_api]
        with self.assertRaisesRegex(AccessError, "Unauthorized attempt to register daemon"):
            self.model.with_user(self.regular.id).register_daemon(
                "Refusal New", self.service.login, "/opt/hams/etc/keys/refusal_new.env"
            )
        self.assertFalse(os.path.exists("/opt/hams/etc/keys/refusal_new.env"))

    def test_a_service_account_cannot_register_a_daemon_for_another_account(self):
        # Tests [@ANCHOR: COMM_register_daemon_api]
        with self.assertRaisesRegex(AccessError, "only provision keys for themselves"):
            self.model.with_user(self.other_service.id).register_daemon(
                "Refusal New", self.service.login, "/opt/hams/etc/keys/refusal_new.env"
            )
        self.assertFalse(os.path.exists("/opt/hams/etc/keys/refusal_new.env"))

    def test_only_a_key_manager_may_change_a_key_lifetime(self):
        # Tests [@ANCHOR: COMM_action_apply_key_lifetime]
        with self.assertRaisesRegex(AccessError, "Only Daemon Key Managers can change a key's lifetime"):
            self.row.with_user(self.regular.id).action_apply_key_lifetime()

    def test_only_a_key_manager_may_rotate_a_key(self):
        # Tests [@ANCHOR: COMM_action_rotate_key_api]
        with self.assertRaisesRegex(AccessError, "Only Daemon Key Managers can rotate keys"):
            self.row.with_user(self.regular.id).action_rotate_key()
        self.assertFalse(os.path.exists(self.path))

    def test_self_rotation_refuses_a_key_that_is_not_a_string(self):
        # Tests [@ANCHOR: COMM_rotate_own_key_api]
        self.row.remote_self_rotation = True
        with self.assertRaisesRegex(AccessError, "Remote self-rotation is not available"):
            self.model.with_user(self.service.id).rotate_own_key("Refusal Row", 12345)

    def test_an_account_in_the_system_group_cannot_hold_a_daemon_key(self):
        # Tests [@ANCHOR: COMM_rotation_safety_archived_user]
        self.service.group_ids = [(4, self.env.ref("base.group_system").id)]
        with self.assertRaisesRegex(UserError, "__system__ user ID cannot be used"):
            self.row.with_user(self.manager.id)._assert_account_may_hold_key()

    def test_a_key_file_is_refused_outside_the_key_directory(self):
        # Tests [@ANCHOR: COMM_write_secure_env_file_logic]
        outside = "/opt/hams/etc/refusal_outside.env"
        with self.assertRaisesRegex(UserError, "must start with"):
            self.row.with_user(self.manager.id)._write_secure_env_file(outside, "login", "key")
        self.assertFalse(os.path.exists(outside))

    def test_an_unexpected_rotation_error_is_recorded_and_does_not_stop_the_cron(self):
        # Tests [@ANCHOR: COMM_cron_rotation_logic]
        old = fields.Datetime.now() - datetime.timedelta(days=90)
        self.row.last_rotated = old
        self.env.cr.commit()

        def boom(reg, pre_fetched_keys=None):
            raise RuntimeError("something nobody planned for")

        self.safe_patch_object(type(self.model), "_rotate_key_and_write_file", boom)
        self.model.with_user(self.manager.id)._cron_rotate_all_keys()
        self.row.invalidate_recordset()
        self.assertTrue(self.row.last_rotation_failed_at)
        self.assertEqual(self.row.last_rotated, old, "a failed rotation is not marked as done")
