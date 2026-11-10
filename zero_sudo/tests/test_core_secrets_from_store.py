# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Odoo core's `database.secret` and Web Push VAPID private key served from the secret store
(zero_sudo/models/core_secrets.py), in both modes: flag off (the current database behaviour, store never
read) and flag on (store only, database row holds the marker). Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Odoo core's secrets")."""

from odoo.exceptions import UserError
from odoo.tests.common import TransactionCase, tagged
from odoo.tools import misc

from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.models import core_secrets as cs
from odoo.addons.zero_sudo.tests.common import SafePatchMixin

DB_SECRET = "db-copy-of-the-test-database-secret"
STORE_SECRET = "store-copy-of-the-test-database-secret"
DB_VAPID = "db-copy-of-a-test-vapid-key"
STORE_VAPID = "store-copy-of-a-test-vapid-key"


class _DictBackend:
    """A Tier A backend over a dict, standing in for the root-owned file a test cannot create."""

    label = "test"

    def __init__(self, values):
        self.values = values

    def get(self, name):
        return self.values.get(ss.check_name(name))


@tagged("post_install", "-at_install")
class TestCoreSecretsFromStore(TransactionCase, SafePatchMixin):
    """TransactionCase, not HamsTransactionCase: zero_sudo is an open-source module and this class needs
    nothing from hams_com (HamsTransactionCase patches ham_base helpers, absent in a zero_sudo-only run)."""

    def setUp(self):
        super().setUp()
        self.icp = self.env["ir.config_parameter"]
        self._set_row(cs.DATABASE_SECRET, DB_SECRET)
        self._set_row(cs.VAPID_PRIVATE_KEY, DB_VAPID)
        self._set_row(cs.VAPID_PUBLIC_KEY, "test-public-key")
        self.store_values = {cs.DATABASE_SECRET: STORE_SECRET, cs.VAPID_PRIVATE_KEY: STORE_VAPID}
        self.store_calls = []

        def fake_store():
            self.store_calls.append(1)
            return ss.SecretStore([_DictBackend(self.store_values)])

        self.safe_patch_object(cs, "core_secret_store", side_effect=fake_store)

    def _set_row(self, key, value):
        """Write a row with SQL (the flag-on guard does not apply) and drop the ormcache."""
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

    def _flag(self, on):
        self.safe_patch_object(cs, "core_secrets_from_store", return_value=on)

    def _session_secret(self):
        values = dict(self.env.user._session_token_get_values())
        return values["database_secret"]

    # ---- the flag ----

    def test_parse_flag(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_parse_flag]
        for text in (None, False, "", "0", "False", "no", " off "):
            self.assertFalse(cs.parse_flag(text), text)
        for text in (True, "1", "True", "YES", "on"):
            self.assertTrue(cs.parse_flag(text), text)
        with self.assertRaises(ValueError):
            cs.parse_flag("ture")

    def test_flag_reads_the_odoo_option(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_from_store]
        self.assertTrue(cs.core_secrets_from_store({cs.FLAG_OPTION: "True"}))
        self.assertFalse(cs.core_secrets_from_store({cs.FLAG_OPTION: "False"}))
        self.assertFalse(cs.core_secrets_from_store({}))
        # No option in this test server's configuration: the database behaviour.
        self.assertFalse(cs.core_secrets_from_store())

    # ---- flag off: today's database behaviour, store never read ----

    def test_off_reads_the_database_and_never_the_store(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_get_param]
        # Tests [@ANCHOR: zero_sudo:core_secrets_session_token_values]
        # Tests [@ANCHOR: zero_sudo:core_secrets_check_store]
        self._flag(False)
        self.assertEqual(self.icp.get_param(cs.DATABASE_SECRET), DB_SECRET)
        self.assertEqual(self.icp.get_param(cs.VAPID_PRIVATE_KEY), DB_VAPID)
        self.assertEqual(self._session_secret(), DB_SECRET)
        self.icp._check_core_secrets_in_store()
        self.assertEqual(self.store_calls, [])

    def test_off_works_without_any_store(self):
        self._flag(False)
        self.store_values.clear()
        self.assertEqual(self.icp.get_param(cs.DATABASE_SECRET), DB_SECRET)
        self.icp._check_core_secrets_in_store()

    def test_off_set_param_stores_the_value(self):
        self._flag(False)
        self.icp.set_param(cs.VAPID_PRIVATE_KEY, "another-test-vapid-key")
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), "another-test-vapid-key")

    def test_off_clear_is_refused(self):
        self._flag(False)
        with self.assertRaises(UserError):
            self.icp._hams_core_secrets_clear_from_database()
        self.assertEqual(self._row(cs.DATABASE_SECRET), DB_SECRET)

    # ---- flag on: store only ----

    def test_on_reads_the_store(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_get_param]
        # Tests [@ANCHOR: zero_sudo:core_secrets_store]
        # Tests [@ANCHOR: zero_sudo:core_secrets_store_value]
        self._flag(True)
        self.assertEqual(self.icp.get_param(cs.DATABASE_SECRET), STORE_SECRET)
        self.assertEqual(self.icp.get_param(cs.VAPID_PRIVATE_KEY), STORE_VAPID)
        self.assertEqual(self.icp.get_param(cs.VAPID_PUBLIC_KEY), "test-public-key")
        self.assertEqual(self.icp.get_param("database.uuid"), self._row("database.uuid"))

    def test_on_core_hmac_signs_with_the_store_value(self):
        self._flag(True)
        self.assertEqual(misc.hmac(self.env, "scope", "message"),
                         misc.hmac(self.env, "scope", "message", secret=STORE_SECRET))
        self.assertNotEqual(misc.hmac(self.env, "scope", "message"),
                            misc.hmac(self.env, "scope", "message", secret=DB_SECRET))

    def test_on_missing_database_secret_is_loud(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_store_value]
        # Tests [@ANCHOR: zero_sudo:core_secrets_get_param]
        # Tests [@ANCHOR: zero_sudo:core_secrets_session_token_values]
        self._flag(True)
        del self.store_values[cs.DATABASE_SECRET]
        with self.assertRaises(ss.SecretNotFound):
            self.icp.get_param(cs.DATABASE_SECRET)
        with self.assertRaises(ss.SecretNotFound):
            self._session_secret()

    def test_on_missing_vapid_key_reads_as_not_configured(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_optional_store_value]
        self._flag(True)
        del self.store_values[cs.VAPID_PRIVATE_KEY]
        self.assertFalse(self.icp.get_param(cs.VAPID_PRIVATE_KEY))

    def test_on_session_token_uses_the_store_value(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_session_token_values]
        self._flag(True)
        self.assertEqual(self._session_secret(), STORE_SECRET)

    def test_flip_keeps_sessions_when_the_store_holds_the_old_value(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_session_token_values]
        sid = "test-session-id"
        user = self.env.user
        self._flag(False)
        before = user._session_token_hash_compute(sid, user._session_token_get_values())
        self.store_values[cs.DATABASE_SECRET] = DB_SECRET
        self._flag(True)
        self._set_row(cs.DATABASE_SECRET, cs.MOVED_MARKER)
        after = user._session_token_hash_compute(sid, user._session_token_get_values())
        self.assertEqual(before, after)

    def test_on_set_param_never_writes_the_secret(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_db_value]
        # Tests [@ANCHOR: zero_sudo:core_secrets_write]
        self._flag(True)
        self.icp.set_param(cs.DATABASE_SECRET, "a-fresh-test-database-secret")
        self.assertEqual(self._row(cs.DATABASE_SECRET), cs.MOVED_MARKER)
        self.icp.set_param(cs.VAPID_PRIVATE_KEY, STORE_VAPID)
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), cs.MOVED_MARKER)

    def test_on_create_never_writes_the_secret(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_create]
        self._flag(True)
        self.env.cr.execute("DELETE FROM ir_config_parameter WHERE key = %s", [cs.DATABASE_SECRET])
        self.env.registry.clear_cache("stable")
        self.icp.set_param(cs.DATABASE_SECRET, "a-fresh-test-database-secret")
        self.assertEqual(self._row(cs.DATABASE_SECRET), cs.MOVED_MARKER)

    def test_on_new_vapid_key_not_in_the_store_is_refused(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_db_value]
        # Tests [@ANCHOR: zero_sudo:core_secrets_write]
        self._flag(True)
        with self.assertRaises(UserError):
            self.icp.set_param(cs.VAPID_PRIVATE_KEY, "a-new-test-vapid-key")
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), DB_VAPID)

    def test_on_non_ascii_vapid_value_is_a_user_error_not_a_type_error(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_db_value]
        self._flag(True)
        with self.assertRaises(UserError):
            self.icp.set_param(cs.VAPID_PRIVATE_KEY, "caf\u00e9-vapid-key")
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), DB_VAPID)

    def test_on_write_of_other_keys_is_untouched(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_write]
        self._flag(True)
        self.icp.set_param("zero_sudo.test_core_secrets_other", "x")
        rows = self.icp.search([("key", "in", [cs.DATABASE_SECRET, "zero_sudo.test_core_secrets_other"])])
        rows.write({"value": "same-test-value"})
        self.assertEqual(self._row("zero_sudo.test_core_secrets_other"), "same-test-value")
        self.assertEqual(self._row(cs.DATABASE_SECRET), cs.MOVED_MARKER)

    # ---- startup check ----

    def test_on_startup_check(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_check_store]
        # Tests [@ANCHOR: zero_sudo:core_secrets_register_hook]
        self._flag(True)
        self.icp._check_core_secrets_in_store()
        del self.store_values[cs.VAPID_PRIVATE_KEY]
        with self.assertRaises(ss.SecretNotFound):
            self.icp._check_core_secrets_in_store()
        self._set_row(cs.VAPID_PUBLIC_KEY, "")
        self.icp._check_core_secrets_in_store()
        del self.store_values[cs.DATABASE_SECRET]
        with self.assertRaises(ss.SecretNotFound):
            self.icp._register_hook()

    # ---- rollout and rollback steps ----

    def test_clear_then_restore(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_clear_from_database]
        # Tests [@ANCHOR: zero_sudo:core_secrets_restore_to_database]
        self._flag(True)
        self.assertEqual(self.icp._hams_core_secrets_clear_from_database(),
                         sorted(cs.CORE_SECRET_KEYS))
        self.assertEqual(self._row(cs.DATABASE_SECRET), cs.MOVED_MARKER)
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), cs.MOVED_MARKER)
        self.assertEqual(self.icp._hams_core_secrets_clear_from_database(), [])
        self.assertEqual(self.icp._hams_core_secrets_restore_to_database(), list(cs.CORE_SECRET_KEYS))
        self.assertEqual(self._row(cs.DATABASE_SECRET), STORE_SECRET)
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), STORE_VAPID)

    def test_clear_refuses_when_the_store_lacks_a_value(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_clear_from_database]
        self._flag(True)
        del self.store_values[cs.VAPID_PRIVATE_KEY]
        with self.assertRaises(ss.SecretNotFound):
            self.icp._hams_core_secrets_clear_from_database()
        self.assertEqual(self._row(cs.DATABASE_SECRET), DB_SECRET)
        self.assertEqual(self._row(cs.VAPID_PRIVATE_KEY), DB_VAPID)

    def test_restore_then_flag_off_reads_the_same_secret(self):
        # Tests [@ANCHOR: zero_sudo:core_secrets_restore_to_database]
        self._flag(True)
        self.icp._hams_core_secrets_clear_from_database()
        self.icp._hams_core_secrets_restore_to_database()
        self._flag(False)
        self.env.registry.clear_cache("stable")
        self.assertEqual(self.icp.get_param(cs.DATABASE_SECRET), STORE_SECRET)
        self.assertEqual(self._session_secret(), STORE_SECRET)
