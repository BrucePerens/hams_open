# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Module secrets served from the secret store by group (zero_sudo/models/module_secrets.py). All values
are test values. Group `stripe`: flag off keeps the database behaviour, flag on serves the store only and
the database never holds the secret. Story: zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets")."""

from odoo.exceptions import UserError
from odoo.tests.common import TransactionCase, tagged

from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.models import module_secrets as ms
from odoo.addons.zero_sudo.models.core_secrets import MOVED_MARKER
from odoo.addons.zero_sudo.tests.common import SafePatchMixin

DB_KEY = "sk_test_database_copy_0000"
STORE_KEY = "sk_test_store_copy_1111"
STORE_WH = "whsec_test_store_copy_2222"


class _DictBackend:
    label = "test"

    def __init__(self, values):
        self.values = values

    def get(self, name):
        return self.values.get(ss.check_name(name))


@tagged("post_install", "-at_install")
class TestModuleSecrets(TransactionCase, SafePatchMixin):

    def setUp(self):
        super().setUp()
        self.icp = self.env["ir.config_parameter"]
        self._set_row("stripe.secret_key", DB_KEY)
        self.store_values = {"stripe.secret_key": STORE_KEY, "stripe.webhook_secret": STORE_WH}
        self.store_calls = []

        def fake_store():
            self.store_calls.append(1)
            return ss.SecretStore([_DictBackend(self.store_values)])

        self.safe_patch_object(ms, "module_secret_store", side_effect=fake_store)

    def _set_row(self, key, value):
        self.env.cr.execute(
            "INSERT INTO ir_config_parameter (key, value) VALUES (%s, %s) "
            "ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value", [key, value])
        self.env.invalidate_all()
        self.env.registry.clear_cache("stable")

    def _row(self, key):
        self.env.flush_all()
        self.env.cr.execute("SELECT value FROM ir_config_parameter WHERE key = %s", [key])
        row = self.env.cr.fetchone()
        return row and row[0]

    def _groups(self, text):
        self.safe_patch_object(ms, "enabled_groups", return_value=ms.enabled_groups({ms.FLAG_OPTION: text}))

    def test_enabled_groups_parse(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_enabled_groups]
        # Tests [@ANCHOR: zero_sudo:module_secrets_group_enabled]
        self.assertEqual(ms.enabled_groups({}), frozenset())
        self.assertEqual(ms.enabled_groups({ms.FLAG_OPTION: " Stripe "}), frozenset({"stripe"}))
        self.assertTrue(ms.group_enabled("stripe", {ms.FLAG_OPTION: "stripe"}))
        self.assertFalse(ms.group_enabled("stripe", {ms.FLAG_OPTION: ""}))
        with self.assertRaises(ValueError):
            ms.enabled_groups({ms.FLAG_OPTION: "stripe,stripee"})

    def test_flag_off_keeps_the_database_and_never_reads_the_store(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_get_param]
        self._groups("")
        self.assertEqual(self.icp._get_param("stripe.secret_key"), DB_KEY)
        self.assertEqual(self.store_calls, [])

    def test_flag_on_serves_the_store(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_get_param]
        # Tests [@ANCHOR: zero_sudo:module_secrets_param_keys]
        # Tests [@ANCHOR: zero_sudo:module_secrets_store_secret]
        # Tests [@ANCHOR: zero_sudo:module_secrets_store]
        # Tests [@ANCHOR: zero_sudo:module_secrets_optional_store_secret]
        # Tests [@ANCHOR: zero_sudo_module_secrets_ir_config_parameter_module_secrets]
        self._groups("stripe")
        self.assertEqual(self.icp._get_param("stripe.secret_key"), STORE_KEY)
        self.assertEqual(self.icp._get_param("stripe.webhook_secret"), STORE_WH)
        self.store_values.clear()
        self.assertIsNone(self.icp._get_param("stripe.secret_key"))

    def test_the_secret_never_reaches_the_database(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_db_value]
        # Tests [@ANCHOR: zero_sudo:module_secrets_write]
        # Tests [@ANCHOR: zero_sudo:module_secrets_create]
        self._groups("stripe")
        self.icp.set_param("stripe.secret_key", STORE_KEY)
        self.assertEqual(self._row("stripe.secret_key"), MOVED_MARKER)
        self.icp.set_param("stripe.webhook_secret", STORE_WH)
        self.assertEqual(self._row("stripe.webhook_secret"), MOVED_MARKER)
        with self.assertRaises(UserError):
            self.icp.set_param("stripe.secret_key", "sk_test_something_else")
        self.assertEqual(self._row("stripe.secret_key"), MOVED_MARKER)
        self.icp.set_param("stripe.secret_key", False)
        # No ir_config_parameter row holds any store value.
        self.env.cr.execute("SELECT key FROM ir_config_parameter WHERE value = ANY(%s)", [[STORE_KEY, STORE_WH]])
        self.assertEqual(self.env.cr.fetchall(), [])

    def test_clear_step_replaces_the_copies_and_refuses_without_the_store_value(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_clear_from_database]
        self._groups("stripe")
        self.store_values.pop("stripe.secret_key")
        with self.assertRaises(ss.SecretNotFound):
            self.icp._hams_module_secrets_clear_from_database()
        self.assertEqual(self._row("stripe.secret_key"), DB_KEY)
        self.store_values["stripe.secret_key"] = STORE_KEY
        self.assertEqual(self.icp._hams_module_secrets_clear_from_database(), ["stripe.secret_key"])
        self.assertEqual(self._row("stripe.secret_key"), MOVED_MARKER)
        self.assertEqual(self.icp._get_param("stripe.secret_key"), STORE_KEY)

    def test_clear_step_needs_a_group(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_clear_from_database]
        self._groups("")
        with self.assertRaises(UserError):
            self.icp._hams_module_secrets_clear_from_database()

    def test_startup_check_fails_when_the_store_lacks_a_key_the_database_has(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_check_store]
        # Tests [@ANCHOR: zero_sudo:module_secrets_register_hook]
        self._groups("stripe")
        self.icp._check_module_secrets_in_store()
        self.store_values.pop("stripe.secret_key")
        with self.assertRaises(ss.SecretNotFound):
            self.icp._check_module_secrets_in_store()
        self._set_row("stripe.secret_key", "")
        self.icp._check_module_secrets_in_store()

    def test_restore_puts_the_store_values_back(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_restore_to_database]
        self._groups("stripe")
        self._set_row("stripe.secret_key", MOVED_MARKER)
        self.assertEqual(self.icp._hams_module_secrets_restore_to_database(), ["stripe.secret_key", "stripe.webhook_secret"])
        self._groups("")
        self.assertEqual(self.icp._get_param("stripe.secret_key"), STORE_KEY)

    def test_ai_group_serves_provider_keys_and_keeps_them_out_of_the_database(self):
        # Tests [@ANCHOR: zero_sudo:module_secrets_get_param]
        # Tests [@ANCHOR: zero_sudo:module_secrets_db_value]
        keys = ("gemini.api_key", "greyline.api_key", "greyline.password")
        self.store_values.update({k: "test-ai-store-" + k for k in keys})
        self._set_row("gemini.api_key", "test-ai-database-copy")
        self._groups("ai")
        self.assertEqual(ms.param_keys(), keys)
        for key in keys:
            self.assertEqual(self.icp._get_param(key), "test-ai-store-" + key)
            self.icp.set_param(key, "test-ai-store-" + key)
            self.assertEqual(self._row(key), MOVED_MARKER)
        with self.assertRaises(UserError):
            self.icp.set_param("gemini.api_key", "test-ai-typed")
        self.assertEqual(self.icp._hams_module_secrets_clear_from_database(), [])
        self.env.cr.execute("SELECT key FROM ir_config_parameter WHERE value LIKE 'test-ai-%'")
        self.assertEqual(self.env.cr.fetchall(), [])
        # Stripe keys are not served when only the ai group is listed.
        self.assertEqual(self.icp._get_param("stripe.secret_key"), DB_KEY)
