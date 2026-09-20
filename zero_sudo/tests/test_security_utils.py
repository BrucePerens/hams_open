# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.tests.common import tagged
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.models import security_utils as security_utils_module
from odoo.exceptions import AccessError, UserError
from unittest.mock import MagicMock, mock_open
import json
import os
import odoo
from odoo.tools import mute_logger
import psycopg2
import logging

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestSecurityUtils(HamsTransactionCase):

    def test_01_mechanical_secret_block_enforcement(self):
        # [@ANCHOR: zero_sudo:COMM_test_01_mechanical_secret_block_enforcement]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_01_mechanical_secret_block_enforcement]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_get_system_param]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_set_system_param]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_parameter_whitelisting]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_securing_configuration]
        """Verify that parameters matching cryptographic patterns are blocked."""
        utils = self.env["zero_sudo.security.utils"]

        # Safe parameters should pass
        base_url = utils._get_system_param("web.base.url")
        self.assertTrue(
            base_url is not None or base_url is False,
            msg="[!] DIAGNOSTIC FOR AI: Failed to retrieve whitelisted parameter 'web.base.url'.",
        )

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

    def test_16_get_system_param_returns_the_real_configured_value(self):
        """
        _get_system_param/_set_system_param's own whitelist check IS their
        security boundary; the actual read/write must not be silently
        degraded by ir_config_parameter.py's separate, narrower
        _SERVICE_ALLOWED_KEYS gate underneath them. Most of
        _get_param_read_whitelist()'s own keys (distributed_redis_cache.*,
        backup_management.*, rabbitmq.*, most user_websites.*, etc.) are
        NOT in that narrower gate, so routing the actual value fetch
        through a service account there used to silently return the
        caller's `default` instead of the real value -- every consumer
        (e.g. redis_pool.py resolving the real Redis host) got a plausible
        wrong answer instead of an error. Prove the round trip actually
        carries the real value, not just that it doesn't crash.
        """
        # Tests [@ANCHOR: zero_sudo:get_param_read_whitelist]

        # Tests [@ANCHOR: zero_sudo:get_param_write_whitelist]
        utils = self.env["zero_sudo.security.utils"]
        # Not in ir_config_parameter.py's _SERVICE_ALLOWED_KEYS and doesn't
        # start with "ham" -- exactly the class of key this bug hit.
        key = "distributed_redis_cache.redis_host"
        self.assertIn(key, utils._get_param_read_whitelist())
        self.assertNotIn(key, utils._get_param_write_whitelist())

        distinct_value = "redis-under-test.internal"
        self.env["ir.config_parameter"].set_param(key, distinct_value)
        self.env.registry.clear_cache()

        got = utils._get_system_param(key)
        self.assertEqual(
            got,
            distinct_value,
            "_get_system_param MUST return the real configured value, not "
            "silently fall back to a default because of an unrelated, "
            "narrower access gate underneath it.",
        )

        # And the write side of the same round trip, for a key that's on
        # the write whitelist too.
        write_key = "caching.safe_quota_mb"
        self.assertIn(write_key, utils._get_param_write_whitelist())
        utils._set_system_param(write_key, "42")
        self.env.registry.clear_cache()
        self.assertEqual(self.env["ir.config_parameter"].get_param(write_key), "42")

    def test_17_get_system_param_works_when_the_caller_is_itself_a_service_account(self):
        """
        Real regression: _get_system_param's internal read routes through
        config_service_internal, which is ITSELF a service account -- so it
        was subject to ir_config_parameter.py's own, separately maintained
        _SERVICE_ALLOWED_KEYS gate, regardless of what the ORIGINAL caller
        of _get_system_param was. That gate only recognized a handful of
        keys, so a key _get_system_param's own whitelist had already
        approved could still be rejected one layer down. Caught by
        user_websites' weekly-digest cron, which calls _get_system_param
        for "user_websites.global_website_page_limit" from within a
        with_user(user_websites_service_account) context. Fixed by making
        ir_config_parameter.py's gate honor this whitelist directly (see
        ham_base/models/ir_config_parameter.py's _service_read_allowed_keys()),
        rather than trying to bypass the gate with sudo()/SUPERUSER_ID,
        which are forbidden on this platform.
        """
        key = "user_websites.global_website_page_limit"
        utils = self.env["zero_sudo.security.utils"]
        self.assertIn(key, utils._get_param_read_whitelist())

        distinct_value = "17"
        self.env["ir.config_parameter"].set_param(key, distinct_value)
        self.env.registry.clear_cache()

        # Any service account exercises the "caller is itself a service account" path;
        # use zero_sudo's OWN mail_service_internal rather than user_websites' account, so
        # this test needs no module beyond zero_sudo's declared dependencies (a bare
        # `-u zero_sudo` never installs user_websites).
        some_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        self.assertTrue(
            self.env["res.users"].browse(some_svc).is_service_account,
            "test setup assumption: this must actually be a service account",
        )
        got = utils.with_user(some_svc)._get_system_param(key)
        self.assertEqual(
            got,
            distinct_value,
            "_get_system_param MUST succeed and return the real value even "
            "when called from within an already-service-account context.",
        )

    # Tests [@ANCHOR: zero_sudo:COMM_zero_sudo_doc_installer]
    def test_02_bdd_ormcache_query_counting_service_uid(self):
        # [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_resolve]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_resolve]
        # ---
        # [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_verify]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_verify]
        # ---
        # [@ANCHOR: zero_sudo:COMM_test_get_service_uid]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_get_service_uid]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_get_service_uid_sql_resolve]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_get_service_uid_sql_verify]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_secure_escalation]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]
        utils = self.env["zero_sudo.security.utils"]

        # We must test using a valid Service Account, as the utility
        # violently rejects human users like 'base.user_admin'
        svc_xml_id = "zero_sudo.mail_service_internal"

        # If this raises AccessError, it means the test env is missing the demo service user.
        # Ensure 'test_tours' data or zero_sudo demo data created it. Assuming it exists:
        utils._get_service_uid(svc_xml_id)

        # Use monkey patch to avoid HamsTransactionCase forbid mock restriction
        original_execute = self.env.cr.execute
        calls = []
        def fake_execute(query, params=None, log_exceptions=True):
            calls.append((query, params))
            return original_execute(query, params, log_exceptions)
        self.env.cr.execute = fake_execute
        try:
            utils._get_service_uid(svc_xml_id)
            for call in calls:
                self.assertNotIn("res_users", call[0])
        finally:
            self.env.cr.execute = original_execute

    def test_03_bdd_event_bus_payload_generation(self):
        # [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_coherent_cache_signal]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_cache_signaling]
        """
        Bug-hunt fix, 2026-09-13: this test used to assert the exact shape of a
        `pg_notify("cache_invalidation", "test.model:test_key")` call -- which locked in a real
        bug (the real listener, `distributed_redis_cache/daemons/cache_manager.py`, LISTENs on
        `"distributed_cache_invalidation"`, not `"cache_invalidation"`, and expects a JSON
        payload, not a plain colon-joined string) rather than catching it, a bug class 2
        (non-discriminating test) instance -- it "passed" whether the signal actually reached
        the real daemon or not, since it only checked what THIS function sent, never what the
        real consumer requires. Rewritten to assert the actual, corrected contract:
        `_notify_cache_invalidation` must delegate to the real
        `distributed_cache_invalidation` channel with a JSON-decodable `{"model": ...}` payload.
        "test.model" is intentionally replaced with a real model name ("res.partner") --
        `notify_model_invalidation()` (the delegate) silently no-ops for any string that isn't a
        real, registered model, which a fake name would trigger and defeat this test's own point.
        """
        utils = self.env["zero_sudo.security.utils"]

        original_execute = self.env.cr.execute
        calls = []
        def fake_execute(query, params=None, log_exceptions=True):
            calls.append((query, params))
            return original_execute(query, params, log_exceptions)

        self.env.cr.execute = fake_execute
        try:
            utils._notify_cache_invalidation("res.partner", "test_key")
            notify_calls = [c for c in calls if c[0] == "SELECT pg_notify(%s, %s)"]
            self.assertEqual(
                len(notify_calls), 1,
                "_notify_cache_invalidation() must issue exactly one pg_notify() call.",
            )
            channel, payload = notify_calls[0][1]
            self.assertEqual(
                channel, "distributed_cache_invalidation",
                "Must notify on the channel distributed_redis_cache/daemons/cache_manager.py "
                "actually LISTENs on -- the OLD 'cache_invalidation' channel has no real "
                "listener anywhere in either repo.",
            )
            decoded = json.loads(payload)
            self.assertEqual(
                decoded.get("model"), "res.partner",
                "Payload must be JSON (the real listener does json.loads() on it) and name "
                "the invalidated model.",
            )

            # Test edge cases
            calls.clear()
            utils._notify_cache_invalidation("", "test_key")
            utils._notify_cache_invalidation("res.partner", "")
            utils._notify_cache_invalidation(None, "test_key")
            utils._notify_cache_invalidation("res.partner", None)
            self.assertEqual(
                len([c for c in calls if c[0] == "SELECT pg_notify(%s, %s)"]),
                0,
                "Should not notify for empty model or key",
            )
        finally:
            self.env.cr.execute = original_execute

    @mute_logger("odoo.sql_db")
    def test_04_privilege_escalation_block_enforcement(self):
        # [@ANCHOR: zero_sudo:COMM_test_privilege_escalation_block_sql]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_privilege_escalation_block_sql]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_privilege_escalation_block_sql]
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
            with self.env.cr.savepoint():
                utils = self.env["zero_sudo.security.utils"]
                utils._get_service_uid("rogue_module.sneaky_admin_service")
            self.fail(
                "Must block Service Accounts with group_system from escalating privileges."
            )
        except (AccessError, UserError, psycopg2.errors.RaiseException) as e:
            self.assertTrue(str(e))

    @mute_logger("odoo.sql_db")
    def test_04b_privilege_escalation_block_enforcement_via_implied_group(self):
        # Tests [@ANCHOR: zero_sudo:COMM_privilege_escalation_block_sql]

        # Bug-hunt fix: the SQL mandate check in postgres_procedures.xml
        # used to query res_groups_users_rel alone, which only sees DIRECT
        # group membership. Odoo's own has_group() checks the full
        # transitive implied-group closure (group_ids.all_implied_ids), so
        # a service account that holds base.group_system only via an
        # IMPLIED chain -- member of some OTHER group whose own
        # implied_ids reaches group_system -- previously passed this check
        # silently, defeating the mandate. This is the same test as
        # test_04_privilege_escalation_block_enforcement above, except the
        # rogue account is a member of a brand-new group that IMPLIES
        # group_system, never group_system directly.
        implying_group = self.env["res.groups"].create(
            {
                "name": "Sneaky Group That Implies Admin",
                "implied_ids": [(4, self.env.ref("base.group_system").id)],
            }
        )
        rogue_user = self.env["res.users"].create(
            {
                "name": "Rogue God Account Via Implication",
                "login": "rogue_god_implied",
                "is_service_account": True,
                "group_ids": [(4, implying_group.id)],
            }
        )
        self.assertIn(
            self.env.ref("base.group_system"),
            rogue_user.all_group_ids,
            "Test setup sanity check: Odoo itself must consider this user a "
            "group_system member via implication before this test means anything.",
        )

        self.env["ir.model.data"].create(
            {
                "module": "rogue_module",
                "name": "sneaky_admin_service_via_implication",
                "model": "res.users",
                "res_id": rogue_user.id,
            }
        )

        try:
            with self.env.cr.savepoint():
                utils = self.env["zero_sudo.security.utils"]
                utils._get_service_uid(
                    "rogue_module.sneaky_admin_service_via_implication"
                )
            self.fail(
                "Must block Service Accounts with an IMPLIED group_system "
                "membership from escalating privileges, not just a direct one."
            )
        except (AccessError, UserError, psycopg2.errors.RaiseException) as e:
            self.assertTrue(str(e))

    def test_05_notify_cache_invalidation_list(self):
        # [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal_batch]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal_batch]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_coherent_cache_signal]
        """
        Test _notify_cache_invalidation with a list payload (the "batch" call shape used by
        real callers like blog_post.py/website_page.py, which pass a list of URLs).

        Bug-hunt fix, 2026-09-13: this test previously asserted the exact `unnest()`-based
        chunking SQL the old, broken implementation used to send multiple per-key payloads on
        the WRONG channel (see test_03's own updated docstring for the full story on why that
        channel was never actually listened to) -- itself a non-discriminating test (bug class
        2), since it verified the shape of a signal nothing downstream ever consumed. The real
        daemon+Redis pipeline only ever operates at whole-MODEL granularity, never per-key, so
        there is nothing left to chunk: rewritten to assert that a list of keys still results in
        exactly ONE correctly-addressed notify for the model (not one per key), matching the
        real, corrected, delegated behavior.
        """
        utils = self.env["zero_sudo.security.utils"]

        original_execute = self.env.cr.execute
        calls = []
        def fake_execute(query, params=None, log_exceptions=True):
            calls.append((query, params))
            return original_execute(query, params, log_exceptions)

        self.env.cr.execute = fake_execute
        try:
            utils._notify_cache_invalidation("res.partner", ["key1", "key2", "key1"])

            notify_calls = [c for c in calls if c[0] == "SELECT pg_notify(%s, %s)"]
            self.assertEqual(
                len(notify_calls), 1,
                "A list of keys must still produce exactly one whole-model notify, not one "
                "per key -- the real daemon+Redis pipeline has no per-key granularity to "
                "chunk for.",
            )
            channel, payload = notify_calls[0][1]
            self.assertEqual(channel, "distributed_cache_invalidation")
            self.assertEqual(json.loads(payload).get("model"), "res.partner")
        finally:
            self.env.cr.execute = original_execute

    def test_06_get_deterministic_hash(self):
        # [@ANCHOR: zero_sudo:COMM_test_deterministic_hash]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_deterministic_hash]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_deterministic_hash]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_deterministic_hash]
        """Verify _get_deterministic_hash generates consistent integer hashes."""
        utils = self.env["zero_sudo.security.utils"]

        hash1 = utils._get_deterministic_hash("test_string_1")
        hash2 = utils._get_deterministic_hash("test_string_1")
        hash3 = utils._get_deterministic_hash("test_string_2")
        hash4 = utils._get_deterministic_hash(12345)

        self.assertIsInstance(hash1, int)
        self.assertEqual(hash1, hash2, "Same input should yield same hash")
        self.assertNotEqual(
            hash1, hash3, "Different inputs should yield different hashes"
        )
        self.assertIsInstance(hash4, int, "Should handle non-string inputs gracefully")
        self.assertTrue(
            0 <= hash1 <= 2147483647, "Hash should be within 32-bit integer range"
        )

    def test_08_get_crypto_secret(self):
        # Tests [@ANCHOR: zero_sudo:safe_patch]

        # Tests [@ANCHOR: zero_sudo:safe_patch_object]

        # Tests [@ANCHOR: zero_sudo:diagnostic_mock_init]

        # Tests [@ANCHOR: zero_sudo:diagnostic_mock_call]
        # (safe_patch()/safe_patch_object() default to DiagnosticMock as
        # new_callable whenever the caller doesn't pass an explicit
        # new=/new_callable= -- both branches used by this exact test.)
        # [@ANCHOR: zero_sudo:COMM_test_get_crypto_secret]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_crypto_secret]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_get_crypto_secret]
        """Test the cryptographic secret retrieval hierarchy."""
        utils = self.env["zero_sudo.security.utils"]
        # Clear cache since the method is ormcache'd
        invalidate_model_cache(utils.env, "zero_sudo.security.utils")
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

        invalidate_model_cache(utils.env, "zero_sudo.security.utils")
        utils.env.registry.clear_cache()

        # 2. Test file fallback
        original_env = os.environ.copy()
        os.environ.clear()
        try:
            self.safe_patch("os.path.exists", return_value=True)
            self.safe_patch("builtins.open", mock_open(read_data="test_file_key\n"))
            self.assertEqual(utils._get_crypto_secret(), "test_file_key")

            # 3. Test configuration fallback
            invalidate_model_cache(utils.env, "zero_sudo.security.utils")
            utils.env.registry.clear_cache()
            self.safe_patch("os.path.exists", return_value=False)
            self.safe_patch_object(
                odoo.tools.config, "get", return_value="test_config_key"
            )
            self.assertEqual(utils._get_crypto_secret(), "test_config_key")
        finally:
            os.environ.clear()
            os.environ.update(original_env)

    def test_09_get_crypto_secret_fails_closed_when_unconfigured(self):
        """
        [!] SECURITY: when none of env var / file / admin_passwd are
        configured, this used to silently substitute the hardcoded
        literal "default_insecure_secret_fallback" -- a value anyone
        reading this open-source repo already knows, making every
        Fernet-encrypted/HMAC-signed value it backed forgeable on an
        unconfigured deployment with no visible failure. Every real
        caller already checks `if not db_secret:` and fails closed, so
        it must return a falsy value here instead of a fake secret.
        """
        utils = self.env["zero_sudo.security.utils"]
        invalidate_model_cache(utils.env, "zero_sudo.security.utils")
        utils.env.registry.clear_cache()

        original_env = os.environ.copy()
        os.environ.clear()
        try:
            self.safe_patch("os.path.exists", return_value=False)
            self.safe_patch_object(odoo.tools.config, "get", return_value=None)
            secret = utils._get_crypto_secret()
            self.assertFalse(
                secret,
                "An unconfigured deployment must get a falsy secret, not "
                "a hardcoded fallback string.",
            )
            self.assertNotEqual(secret, "default_insecure_secret_fallback")
        finally:
            os.environ.clear()
            os.environ.update(original_env)

    def test_10_get_service_env(self):
        # Tests [@ANCHOR: zero_sudo:get_service_env]
        """Verify _get_service_env correctly disables tracking and prefetching."""
        utils = self.env["zero_sudo.security.utils"]
        svc_xml_id = "zero_sudo.mail_service_internal"

        expected_uid = utils._get_service_uid(svc_xml_id)

        env_svc = utils._get_service_env(svc_xml_id)

        # Ensure environment switches cleanly
        self.assertEqual(env_svc.user.id, expected_uid)

        # ADR-0001: Ensure background context overrides exist to prevent nested cache faults
        self.assertTrue(env_svc.context.get("mail_notrack"))

    def test_10b_get_service_env_resets_company_context_not_caller_leak(self):
        """Bug-hunt fix, 2026-09-18, per Bruce's own answer in
        night_shift_questions/answered/
        zero-sudo-get-service-env-company-context-carryover-a133d1a3.md: _get_service_env
        used to inherit whatever allowed_company_ids the CALLER's own ambient context had
        (via with_user() alone), rather than resetting to the impersonated service
        account's own default company (ADR-0083 decision 1: .with_company() is the
        architecturally mandated abstraction). A caller acting under a child company must
        not leak that company into the returned service env."""
        # Tests [@ANCHOR: zero_sudo:get_service_env]
        child_company = self.env["res.company"].create({"name": "Child Co (test)"})
        utils = self.env["zero_sudo.security.utils"]
        svc_xml_id = "zero_sudo.mail_service_internal"
        service_uid = utils._get_service_uid(svc_xml_id)
        service_default_company = (
            self.env["res.users"].browse(service_uid).company_id
        )
        self.assertNotEqual(
            child_company.id,
            service_default_company.id,
            "test setup requires a company distinct from the service account's own default",
        )

        caller = utils.with_company(child_company.id)
        env_svc = caller._get_service_env(svc_xml_id)

        self.assertEqual(
            env_svc.company.id,
            service_default_company.id,
            "the service env's company must be the service account's own default, not "
            "the calling env's child company",
        )
        self.assertNotIn(
            child_company.id,
            env_svc.context.get("allowed_company_ids") or [],
            "the caller's own child company must not leak into the service env's "
            "allowed_company_ids",
        )

    def test_11_ensure_executable(self):
        # Tests [@ANCHOR: zero_sudo:ensure_executable]
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
        mock_env.__getitem__.side_effect = lambda k: (
            mock_manifest if k == "binary.manifest" else None
        )

        patcher = self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=mock_env,
        )
        patcher.start()
        try:
            res = utils._ensure_executable(
                "kopia", svc_xml_id="zero_sudo.mail_service_internal"
            )
            self.assertEqual(res, "/var/lib/odoo/hams_bin/kopia")
            mock_manifest.ensure_executable.assert_called_once_with("kopia")
        finally:
            patcher.stop()

    def test_12_kv_store(self):
        # Tests [@ANCHOR: zero_sudo:get_kv]
        # ---
        # [@ANCHOR: zero_sudo:COMM_test_set_kv_procedure]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_set_kv_procedure]
        # ---
        # [@ANCHOR: zero_sudo:COMM_test_set_kv_sql_check]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_set_kv_sql_check]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_set_kv_procedure]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_set_kv_sql_check]
        """Verify the lightweight Service Account Key-Value storage abstraction."""
        utils = self.env["zero_sudo.security.utils"]

        utils._get_service_uid("zero_sudo.odoo_facility_service_internal")

        # Test Write & Read
        utils._set_kv("test_key_1", "test_value")
        self.assertEqual(utils._get_kv("test_key_1"), "test_value")

        # Test Update
        utils._set_kv("test_key_1", "updated_value")
        self.assertEqual(utils._get_kv("test_key_1"), "updated_value")
        # Tests [@ANCHOR: zero_sudo:COMM_zero_sudo_kv_global]

    def test_noisy_table_global(self):
        # Tests [@ANCHOR: zero_sudo:COMM_zero_sudo_noisy_table_global]
        # We ensure the model is accessible and we can create a record.
        table_name = "test_noisy_table"
        table = self.env["zero_sudo.noisy_table"].create({"name": table_name})
        self.assertTrue(table.exists())
        self.assertIn(
            table_name, self.env["zero_sudo.noisy_table"].search([], limit=10000).mapped("name")
        )

    def test_missing_key(self):
        utils = self.env["zero_sudo.security.utils"]
        # Test Missing Key
        self.assertIsNone(utils._get_kv("non_existent_key"))

    @mute_logger("odoo.sql_db")
    def test_13_service_uid_error_paths(self):
        """Audit all rejection branches within the service account lookup logic.

        Bug-hunt re-verification, 2026-09-18 (night_shift_todo/medium/get-service-uid-raise-
        exception-uncaught-across-callsites-e94d940f.md): this used to tolerate a raw
        psycopg2.errors.RaiseException here too, which would have let a regression of the
        2026-09-09 fix below pass silently -- every caller across both repos writes `except
        AccessError` around _get_service_uid() specifically because it's documented to always
        raise that, never the raw Postgres exception (see _get_service_uid's own
        `except psycopg2.errors.RaiseException as e: raise AccessError(...) from e` -- confirmed
        still present and correct by reading the current source directly, not assumed from this
        test alone). Asserting only AccessError here is what actually proves that promise holds."""
        utils = self.env["zero_sudo.security.utils"]

        # 1. Invalid XML ID Format
        try:
            with self.env.cr.savepoint():
                utils._get_service_uid("invalid_format_no_dot")
            self.fail("Expected exception")
        except AccessError:
            _logger.info("Caught expected exception for missing UID")

        # 2. Account Not Found
        try:
            with self.env.cr.savepoint():
                utils._get_service_uid("base.non_existent_xml_id")
            self.fail("Expected exception")
        except AccessError:
            _logger.info("Caught expected exception for Account Not Found")

        # 3. Deny Human Admin Pass-through
        try:
            with self.env.cr.savepoint():
                utils._get_service_uid("base.user_admin")
            self.fail("Expected exception")
        except AccessError:
            _logger.info("Caught expected exception for human admin pass-through")

        # 4. Deny Disabled Accounts
        disabled_user = self.env["res.users"].create(
            {
                "name": "Disabled SA",
                "login": "disabled_sa",
                "is_service_account": True,
                "active": False,
            }
        )
        self.env["ir.model.data"].create(
            {
                "module": "test_module",
                "name": "disabled_sa_xml",
                "model": "res.users",
                "res_id": disabled_user.id,
            }
        )
        try:
            with self.env.cr.savepoint():
                utils._get_service_uid("test_module.disabled_sa_xml")
            self.fail("Expected exception")
        except AccessError:
            _logger.info("Caught expected exception for disabled accounts")

    def test_14_service_account_password_generation(self):
        # [@ANCHOR: zero_sudo:COMM_test_service_account_password]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_service_account_password]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_is_service_account_field]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_service_account_password_generation]
        """
        Verify that service accounts are automatically assigned a massive,
        cryptographically secure random password to prevent interactive logins.
        """
        service_account_1 = self.env["res.users"].create(
            {
                "name": "Service Account 1",
                "login": "service_test_user_1",
                "is_service_account": True,
            }
        )

        service_account_2 = self.env["res.users"].create(
            {
                "name": "Service Account 2",
                "login": "service_test_user_2",
                "is_service_account": True,
            }
        )

        self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_service_account_password]  # fmt: skip
            "SELECT id, password FROM res_users WHERE id IN %s", ((service_account_1.id, service_account_2.id),)
        )
        results = dict(self.env.cr.fetchall())
        hash_1 = results[service_account_1.id]
        hash_2 = results[service_account_2.id]

        self.assertTrue(hash_1, "Service account MUST have a generated password hash.")
        self.assertTrue(hash_2, "Service account MUST have a generated password hash.")
        self.assertNotEqual(
            hash_1,
            hash_2,
            "Every service account MUST receive a unique random password.",
        )

    def test_15_invalidate_model_cache(self):
        # [@ANCHOR: zero_sudo:COMM_test_invalidate_model_cache]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_invalidate_model_cache]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_invalidate_model_cache]
        """
        Verify secure record-level cache invalidation for specific models.

        Bug-hunt fix, 2026-09-13: this test used to patch `self.env.registry.clear_cache` and
        assert IT was called -- which is exactly the mechanism this pass found to be the real
        bug (`registry.clear_cache()` with no arguments clears the whole 'default' ormcache
        bucket for EVERY model, not `model_name`, contradicting this function's own name and
        docstring). A passing assertion on the buggy mechanism is itself a bug class 2
        (non-discriminating test) instance -- it couldn't have told the difference between the
        real, model-scoped `invalidate_model_cache()` and the old, unscoped
        `registry.clear_cache()`. Rewritten to assert the corrected mechanism directly: the
        model-scoped `invalidate_model_cache()` (imported at the top of this file) is called
        with `model_name`, and `registry.clear_cache()` is NOT called at all (there is no
        remaining reason for this function to touch the registry-wide ormcache bucket).
        """
        utils = self.env["zero_sudo.security.utils"]

        # 1. Admin should be able to invalidate any model cache
        mock_clear_cache = self.safe_patch_object(self.env.registry, "clear_cache")
        mock_invalidate = self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.invalidate_model_cache"
        )
        mock_invalidate.start()
        self.addCleanup(mock_invalidate.stop)
        mock_notify = self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._notify_cache_invalidation"
        )
        mock_notify.start()
        self.addCleanup(mock_notify.stop)

        utils._invalidate_model_cache("res.partner")
        mock_invalidate.assert_called_once_with(utils.env, "res.partner")
        mock_notify.assert_called_once_with("res.partner", "CLEAR_ALL")
        mock_clear_cache.assert_not_called()

        # 2. Non-admin with write access should be able to invalidate
        # We need a user with some write access but not system.
        # Let's create one.
        test_user = self.env["res.users"].create(
            {
                "name": "Test Cache User",
                "login": "test_cache_user",
                "email": "test@test.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        # Odoo's own res.users create() above legitimately calls registry.clear_cache()
        # and clear_cache('stable') (res_users.py write() / ir.model.access
        # call_cache_clearing_methods() on any group change) -- that is the fixture's
        # setup, not _invalidate_model_cache()'s behaviour. Forget those calls so the
        # not-called assertion at the end of this test covers exactly the
        # _invalidate_model_cache() calls below (same strength as before: any call to
        # registry.clear_cache from them still fails the test).
        mock_clear_cache.reset_mock()

        # Portal user usually doesn't have write access to res.partner
        with self.assertRaises(AccessError):
            utils.with_user(test_user)._invalidate_model_cache("res.partner")

        # Give the user a group that has write access to some model
        # For simplicity, let's use a mock check_access.
        # We need to patch check_access on the model class or instance.
        # Since it's Odoo 19, let's try patching it on the recordset.
        mock_check = self.safe_patch("odoo.models.BaseModel.check_access", return_value=True)
        mock_check.start()
        self.addCleanup(mock_check.stop)

        count_before = mock_invalidate.call_count
        utils.with_user(test_user)._invalidate_model_cache("res.partner")
        self.assertEqual(mock_invalidate.call_count, count_before + 1)
        mock_clear_cache.assert_not_called()

    def test_17b_caller_module_name_walks_the_real_stack(self):
        # Tests [@ANCHOR: zero_sudo:caller_module_name]
        # Every _resolve_dependency_cycle test below mocks this method
        # out entirely -- none of them exercise its own real
        # stack-walking/__manifest__.py-discovery logic. Called for real,
        # unmocked, directly from this test method (itself a file inside
        # zero_sudo/tests/, one directory below zero_sudo/__manifest__.py),
        # it must resolve to "zero_sudo".
        utils = self.env["zero_sudo.security.utils"]
        self.assertEqual(utils._caller_module_name(), "zero_sudo")

    def test_17c_caller_module_name_rejects_interactive_shell_frame(self):
        # Tests [@ANCHOR: zero_sudo:caller_module_name]
        """
        A call originating from an interactive shell/console (python3
        REPL, `odoo shell`, `python3 -c`, exec()'d strings) has a
        pseudo-frame filename like "<stdin>" or "<console>" instead of
        a real file path. _caller_module_name() must recognize that and
        return None (indeterminate caller) rather than resolving
        os.path.abspath("<stdin>") to "<cwd>/<stdin>" and walking
        upward from the current working directory -- which would
        silently misattribute the call to whatever module the cwd
        happens to be inside, regardless of what code actually made the
        call.

        Regression test for a real bug found during 2026-09-08 patent-
        disclosure Fable reviews (disclosure 19): before the fix, this
        method joined a pseudo-frame's filename with cwd without first
        checking whether it was a real file, so an interactive-shell
        caller was attributed to the cwd's module instead of being
        rejected as indeterminate.
        """
        utils = self.env["zero_sudo.security.utils"]

        # _caller_module_name() first skips frames whose file is
        # security_utils.py itself, then examines the first remaining
        # frame as "the immediate caller". Standing in for that
        # immediate caller with a single pseudo-frame (filename
        # "<stdin>", as a real interactive shell/console frame would
        # have) is sufficient to exercise the check regardless of what
        # this test file's own real frame looks like.
        fake_frame_info = MagicMock()
        fake_frame_info.filename = "<stdin>"

        # Pin cwd to this test file's own directory (a real subdirectory
        # of zero_sudo, which does have a __manifest__.py up its tree)
        # so a pre-fix misattribution-via-cwd would be caught here: if
        # the isfile() guard were missing, os.path.abspath("<stdin>")
        # would resolve against this cwd and the walk-upward logic
        # would incorrectly return "zero_sudo".
        tests_dir = os.path.dirname(os.path.abspath(__file__))
        self.safe_patch("os.getcwd", return_value=tests_dir)
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.inspect.stack",
            return_value=[fake_frame_info],
        )

        self.assertIsNone(
            utils._caller_module_name(),
            "An interactive-shell-style pseudo-frame (filename "
            "'<stdin>') must be rejected as an indeterminate caller, "
            "not resolved via the current working directory.",
        )

    def test_18_resolve_dependency_cycle_declared_and_installed(self):
        # Tests [@ANCHOR: zero_sudo:resolve_dependency_cycle]
        """
        _resolve_dependency_cycle() is how a module handles depending on
        another module it can't hard-depend on without closing a manifest
        dependency cycle (see hams_shared/tools/check_dependency_cycles.py
        and each such module's own 'depends_cycle' manifest key). The
        relationship must be declared in the CALLING module's own
        manifest -- resolved from the Python call stack, not a
        caller-supplied string -- so this can't be used to silently probe
        an arbitrary, undeclared module.
        """
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils), "_caller_module_name", return_value="fake_caller_module"
        )
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.odoo_get_manifest",
            return_value={"depends_cycle": ["zero_sudo"]},
        )
        # zero_sudo itself is necessarily installed -- this test is
        # running as part of its own test suite.
        self.assertTrue(utils._resolve_dependency_cycle("zero_sudo"))

    def test_19_resolve_dependency_cycle_not_installed_degrades_gracefully(self):
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils), "_caller_module_name", return_value="fake_caller_module"
        )
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.odoo_get_manifest",
            return_value={"depends_cycle": ["definitely_not_a_real_module_xyz"]},
        )
        self.assertFalse(
            utils._resolve_dependency_cycle("definitely_not_a_real_module_xyz"),
            "A missing, declared soft dependency MUST return False, not "
            "raise, when required=False (the default).",
        )

    def test_20_resolve_dependency_cycle_not_installed_and_required_raises(self):
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils), "_caller_module_name", return_value="fake_caller_module"
        )
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.odoo_get_manifest",
            return_value={"depends_cycle": ["definitely_not_a_real_module_xyz"]},
        )
        with self.assertRaises(
            UserError,
            msg="required=True MUST raise loudly rather than silently "
            "letting the caller proceed without its dependency.",
        ):
            utils._resolve_dependency_cycle(
                "definitely_not_a_real_module_xyz", required=True
            )

    def test_21_resolve_dependency_cycle_undeclared_is_rejected(self):
        """
        A module cannot use _resolve_dependency_cycle() to probe a module
        it never declared in its own 'depends_cycle' -- that would let
        the manifest silently drift out of sync with what the code
        actually relies on.
        """
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils), "_caller_module_name", return_value="fake_caller_module"
        )
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.odoo_get_manifest",
            return_value={"depends_cycle": []},
        )
        with self.assertRaises(UserError):
            utils._resolve_dependency_cycle("zero_sudo")

    def test_22_resolve_dependency_cycle_unknown_caller_fails_fast(self):
        """
        If the calling module can't be identified at all, that's not a
        "dependency missing, degrade gracefully" case -- it means the
        'depends_cycle' declaration can't be verified, so this must fail
        fast rather than silently letting an unverifiable caller through.
        """
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils), "_caller_module_name", return_value=None
        )
        with self.assertRaises(UserError):
            utils._resolve_dependency_cycle("zero_sudo")

    def _make_erasure_test_service_account(self, xml_name, group, extra_group=None):
        groups = [(6, 0, [group.id] + ([extra_group.id] if extra_group else []))]
        user = self.env["res.users"].create(
            {
                "name": f"Erasure Test Svc {xml_name}",
                "login": f"erasure_test_svc_{xml_name}",
                "is_service_account": True,
                "active": True,
                "group_ids": groups,
            }
        )
        self.env["ir.model.data"].create(
            {
                "module": "test_module",
                "name": xml_name,
                "model": "res.users",
                "res_id": user.id,
            }
        )
        return user

    def test_23_erase_via_service_account_raises_when_visibility_is_restricted(self):
        # Tests [@ANCHOR: zero_sudo:ground_truth_ids]
        # Reproduces the exact bug class found live in ham_relay_bridge: a
        # service account has real ir.model.access.csv rights on the model,
        # but its own group membership ALSO matches an unrelated, restrictive
        # ir.rule -- so its own search() silently sees fewer records than
        # actually exist, and a hand-rolled search()+unlink() would have
        # silently done nothing. _erase_via_service_account must raise
        # instead.
        utils = self.env["zero_sudo.security.utils"]
        group = self.env["res.groups"].create({"name": "Erasure Test Group 23"})
        self.env["ir.model.access"].create(
            {
                "name": "erasure test 23 partner access",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "group_id": group.id,
                "perm_read": True,
                "perm_write": True,
                "perm_create": True,
                "perm_unlink": True,
            }
        )
        # A restrictive rule that matches nothing real for this group --
        # simulates the unrelated "your own records only" shape.
        self.env["ir.rule"].create(
            {
                "name": "Erasure Test 23 Restrictive Rule",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "domain_force": "[('id', '=', 0)]",
                "groups": [(6, 0, [group.id])],
            }
        )
        self._make_erasure_test_service_account("erasure_svc_23", group)
        partner = self.env["res.partner"].create({"name": "Erasure Test Target 23"})

        with self.assertRaises(AccessError):
            utils._erase_via_service_account(
                "res.partner", [("id", "=", partner.id)], "test_module.erasure_svc_23"
            )

        # And it must NOT have been deleted -- the whole point is refusing to
        # silently proceed with a partial/empty result.
        self.assertTrue(partner.exists())

    def test_24_erase_via_service_account_deletes_when_visibility_matches(self):
        # Tests [@ANCHOR: zero_sudo:erase_via_service_account]
        utils = self.env["zero_sudo.security.utils"]
        group = self.env["res.groups"].create({"name": "Erasure Test Group 24"})
        self.env["ir.model.access"].create(
            {
                "name": "erasure test 24 partner access",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "group_id": group.id,
                "perm_read": True,
                "perm_write": True,
                "perm_create": True,
                "perm_unlink": True,
            }
        )
        # An unrestricted rule for this group -- visibility matches ground truth.
        self.env["ir.rule"].create(
            {
                "name": "Erasure Test 24 Unrestricted Rule",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "domain_force": "[(1, '=', 1)]",
                "groups": [(6, 0, [group.id])],
            }
        )
        self._make_erasure_test_service_account("erasure_svc_24", group)
        partner = self.env["res.partner"].create({"name": "Erasure Test Target 24"})
        partner_id = partner.id

        deleted_ids = utils._erase_via_service_account(
            "res.partner", [("id", "=", partner_id)], "test_module.erasure_svc_24"
        )

        self.assertEqual(deleted_ids, [partner_id])
        self.assertFalse(self.env["res.partner"].search([("id", "=", partner_id)]))

    def test_25_anonymize_via_service_account_raises_when_visibility_is_restricted(self):
        # Tests [@ANCHOR: zero_sudo:anonymize_via_service_account]
        # Write-based sibling of test_23 -- same restricted-visibility shape, but
        # for _anonymize_via_service_account's reassign-ownership path instead of
        # a delete.
        utils = self.env["zero_sudo.security.utils"]
        group = self.env["res.groups"].create({"name": "Anonymize Test Group 25"})
        self.env["ir.model.access"].create(
            {
                "name": "anonymize test 25 partner access",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "group_id": group.id,
                "perm_read": True,
                "perm_write": True,
                "perm_create": True,
                "perm_unlink": True,
            }
        )
        self.env["ir.rule"].create(
            {
                "name": "Anonymize Test 25 Restrictive Rule",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "domain_force": "[('id', '=', 0)]",
                "groups": [(6, 0, [group.id])],
            }
        )
        self._make_erasure_test_service_account("anonymize_svc_25", group)
        original_owner = self.env["res.users"].create(
            {"name": "Original Owner 25", "login": "original_owner_25"}
        )
        partner = self.env["res.partner"].create(
            {"name": "Anonymize Test Target 25", "user_id": original_owner.id}
        )

        with self.assertRaises(AccessError):
            utils._anonymize_via_service_account(
                "res.partner",
                [("id", "=", partner.id)],
                "user_id",
                "test_module.anonymize_svc_25",
            )

        # Ownership must NOT have changed -- refusing to silently proceed with a
        # partial/empty result is the whole point.
        partner.invalidate_recordset()
        self.assertEqual(partner.user_id.id, original_owner.id)

    def test_27_is_test_mode_true_inside_a_transaction_case(self):
        # Tests [@ANCHOR: zero_sudo:is_test_mode]
        """
        This test (HamsTransactionCase, i.e. plain TransactionCase) runs
        against an ordinary cursor whose `.commit` Odoo's own test
        framework patches to raise -- not Odoo's TestCursor, which only
        HttpCase-style tests use -- so _is_test_mode() must still report
        True here. This is the real signal that replaced an older idiom
        this codebase's own check_registry_test_cr_usage.py flags as
        dead code across the codebase.
        """
        utils = self.env["zero_sudo.security.utils"]
        self.assertTrue(utils._is_test_mode())
        self.assertEqual(self.env.cr.commit.__name__, "forbidden")

    def test_26_anonymize_via_service_account_reassigns_when_visibility_matches(self):
        utils = self.env["zero_sudo.security.utils"]
        group = self.env["res.groups"].create({"name": "Anonymize Test Group 26"})
        self.env["ir.model.access"].create(
            {
                "name": "anonymize test 26 partner access",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "group_id": group.id,
                "perm_read": True,
                "perm_write": True,
                "perm_create": True,
                "perm_unlink": True,
            }
        )
        self.env["ir.rule"].create(
            {
                "name": "Anonymize Test 26 Unrestricted Rule",
                "model_id": self.env["ir.model"]._get_id("res.partner"),
                "domain_force": "[(1, '=', 1)]",
                "groups": [(6, 0, [group.id])],
            }
        )
        self._make_erasure_test_service_account("anonymize_svc_26", group)
        original_owner = self.env["res.users"].create(
            {"name": "Original Owner 26", "login": "original_owner_26"}
        )
        partner = self.env["res.partner"].create(
            {"name": "Anonymize Test Target 26", "user_id": original_owner.id}
        )
        partner_id = partner.id

        reassigned_ids = utils._anonymize_via_service_account(
            "res.partner",
            [("id", "=", partner_id)],
            "user_id",
            "test_module.anonymize_svc_26",
        )

        self.assertEqual(reassigned_ids, [partner_id])
        orphan_uid = utils._get_service_uid("zero_sudo.orphaned_record_owner")
        partner.invalidate_recordset()
        self.assertEqual(partner.user_id.id, orphan_uid)

    def _mock_request_obj(self, remote_addr, headers=None):
        mock_obj = MagicMock()
        mock_obj.httprequest.remote_addr = remote_addr
        mock_obj.httprequest.headers = headers or {}
        return mock_obj

    def test_28_get_trusted_client_ip_trusts_cf_header_from_loopback(self):
        # Tests [@ANCHOR: zero_sudo:get_trusted_client_ip]
        """This deployment is Cloudflare-Tunnel-only (confirmed by Bruce
        2026-09-11) -- cloudflared and Odoo run on the same host, so a
        loopback remote_addr IS the tunnel, and its CF-Connecting-IP
        header is genuine."""
        utils = self.env["zero_sudo.security.utils"]
        mock_obj = self._mock_request_obj(
            "127.0.0.1", {"CF-Connecting-IP": "8.8.4.4"}  # burn-ignore-ssrf-test-value
        )
        self.assertEqual(utils._get_trusted_client_ip(mock_obj), "8.8.4.4")

    def test_28b_get_trusted_client_ip_trusts_x_forwarded_for_from_loopback(self):
        # Tests [@ANCHOR: zero_sudo:get_trusted_client_ip]
        utils = self.env["zero_sudo.security.utils"]
        mock_obj = self._mock_request_obj(
            "::1", {"X-Forwarded-For": "8.8.4.4, 10.0.0.1"}
        )
        self.assertEqual(
            utils._get_trusted_client_ip(mock_obj),
            "8.8.4.4",
            "Only the first (real client) hop of a comma-separated "
            "X-Forwarded-For chain must be used.",
        )

    def test_28c_get_trusted_client_ip_ignores_forged_headers_from_a_non_loopback_peer(self):
        # Tests [@ANCHOR: zero_sudo:get_trusted_client_ip]
        """Bug-hunt fix, 2026-09-11: a request whose real transport peer is
        NOT the local Tunnel must never trust CF-Connecting-IP/
        X-Forwarded-For -- they're attacker-controlled on any path that
        doesn't go through cloudflared. The real peer address must be
        returned instead, even though a forged header is present."""
        utils = self.env["zero_sudo.security.utils"]
        mock_obj = self._mock_request_obj(
            "203.0.113.5",
            {"CF-Connecting-IP": "1.2.3.4", "X-Forwarded-For": "5.6.7.8"},
        )
        self.assertEqual(utils._get_trusted_client_ip(mock_obj), "203.0.113.5")

    def test_28d_get_trusted_client_ip_falls_back_to_remote_addr_with_no_headers(self):
        # Tests [@ANCHOR: zero_sudo:get_trusted_client_ip]
        utils = self.env["zero_sudo.security.utils"]
        mock_obj = self._mock_request_obj("127.0.0.1", {})  # burn-ignore-ssrf-test-value
        self.assertEqual(
            utils._get_trusted_client_ip(mock_obj), "127.0.0.1"  # burn-ignore-ssrf-test-value
        )

    def test_28e_get_trusted_client_ip_returns_none_with_no_active_request(self):
        # Tests [@ANCHOR: zero_sudo:get_trusted_client_ip]
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.request", new=None
        )
        self.assertIsNone(utils._get_trusted_client_ip())

    def test_29_preload_service_uid_cache_populates_known_service_accounts_only(self):
        # Tests [@ANCHOR: zero_sudo:preload_service_uid_cache]
        """Bruce's own direct instruction (2026-09-12/13): "load the service user-IDs into a
        hash-table at start-up." _register_hook() (which every real server startup already
        calls once per registry build, before this test even ran) should have already populated
        the cache -- this proves it actually contains a real, known service account's xml_id
        mapped to its real uid, and does NOT contain a real human admin's xml_id (base.user_admin
        is never is_service_account=True, so the preload query's own WHERE clause must exclude
        it)."""
        # Bug-hunt fix, 2026-09-13: _SERVICE_UID_CACHE was re-keyed from bare xml_id to
        # (dbname, xml_id) in the same-day security_utils.py bug-hunt pass (see
        # _preload_service_uid_cache's own module-level comment for the cross-database
        # collision this closes), but this test's own assertions were never updated to match --
        # confirmed live: this assertion started failing the moment that fix landed, looking up
        # a bare-string key against a dict now keyed by 2-tuples.
        dbname = self.env.cr.dbname
        utils = self.env["zero_sudo.security.utils"]
        real_uid = utils._get_service_uid("zero_sudo.mail_service_internal")
        self.assertEqual(
            security_utils_module._SERVICE_UID_CACHE.get((dbname, "zero_sudo.mail_service_internal")),
            real_uid,
            "A real, already-installed service account must be preloaded into the cache by "
            "_register_hook(), keyed to its own real (dbname, xml_id) pair.",
        )
        self.assertNotIn(
            (dbname, "base.user_admin"),
            security_utils_module._SERVICE_UID_CACHE,
            "A real human admin user must never appear in the service-uid cache, even though "
            "it has a real ir.model.data xml_id -- the preload query's own is_service_account "
            "filter must exclude it.",
        )

    def test_30_get_service_uid_cache_hit_skips_ir_model_data_but_still_verifies_live(self):
        # Tests [@ANCHOR: zero_sudo:COMM_get_service_uid]
        """The cache-hit path (a real, preloaded service account) must still return the correct
        uid -- proving the split into zero_sudo_resolve_service_xmlid()/
        zero_sudo_verify_service_uid() didn't change the outward-facing result for the common
        case, only which SQL function actually runs underneath."""
        # Bug-hunt fix, 2026-09-13: same tuple-key mismatch as test_29's own fix above -- see
        # that test's comment for the full story.
        utils = self.env["zero_sudo.security.utils"]
        xml_id = "zero_sudo.mail_service_internal"
        self.assertIn(
            (self.env.cr.dbname, xml_id),
            security_utils_module._SERVICE_UID_CACHE,
            "Test setup assumption: this account must actually be preloaded, or this test "
            "cannot tell a cache hit apart from a miss.",
        )
        uid = utils._get_service_uid(xml_id)
        self.assertTrue(
            self.env["res.users"].browse(uid).is_service_account,
            "The uid returned via the cache-hit path must be the real, correct service account.",
        )

    def test_31_get_service_uid_cache_hit_still_fails_live_if_disabled_after_preload(self):
        # Tests [@ANCHOR: zero_sudo:COMM_get_service_uid]
        """The single most important correctness property of this whole cache: a service
        account that WAS valid at preload time but has since been disabled (a real admin action,
        mid-server-lifetime) must still be rejected live, not silently trusted because its uid
        sits in the process-level cache. Proves the safety verdict (active/is_service_account/
        no privilege escalation) is never itself cached -- only the xml_id->uid resolution is.

        Uses a dedicated, test-local account rather than a real shared one (e.g.
        zero_sudo.mail_service_internal, used pervasively elsewhere in this exact suite):
        _SERVICE_UID_CACHE is a module-level, process-lifetime dict, not scoped to this test's
        own transaction, so disabling a real shared account here -- even with a try/finally
        restore -- risks other tests in the same worker observing it disabled if anything
        (test ordering, an aborted run) skips the restore. Manually seeding the cache with a
        fresh, throwaway account's own real uid reproduces the exact "cached at preload time"
        precondition this test needs, without touching anything another test depends on."""
        fresh_user = self.env["res.users"].create(
            {
                "name": "Cache Staleness Test Service Account",
                "login": "cache_staleness_test_service_account",
                "is_service_account": True,
            }
        )
        xml_id_name = "cache_staleness_test_service_account_xmlid"
        self.env["ir.model.data"].create(
            {
                "module": "test_module",
                "name": xml_id_name,
                "model": "res.users",
                "res_id": fresh_user.id,
            }
        )
        xml_id = f"test_module.{xml_id_name}"

        # Simulate "this account was already valid and cached at this worker's own startup" --
        # a real preload never sees a freshly-created record like this one, so the cache has to
        # be seeded directly to reproduce the precondition this test is about.
        # Bug-hunt fix, 2026-09-13: keyed by bare xml_id before this fix -- since
        # _SERVICE_UID_CACHE was re-keyed to (dbname, xml_id) the same day, a seed under the old
        # bare-string shape was silently invisible to the real _get_service_uid() lookup, so this
        # test was actually exercising the live-resolve fallback path the whole time, not the
        # cache-hit-then-disabled precondition it claims to (both happen to raise for the disabled
        # account either way, which is why this didn't show up as an outright failure).
        cache_key = (self.env.cr.dbname, xml_id)
        security_utils_module._SERVICE_UID_CACHE[cache_key] = fresh_user.id
        try:
            utils = self.env["zero_sudo.security.utils"]
            uid = utils._get_service_uid(xml_id)
            self.assertEqual(
                uid,
                fresh_user.id,
                "Sanity check: the seeded cache entry must resolve correctly while the "
                "account is still genuinely valid, before this test disables it.",
            )

            fresh_user.write({"active": False})
            try:
                with self.env.cr.savepoint():
                    utils._get_service_uid(xml_id)
                self.fail(
                    "A cached service account that was disabled AFTER preload must still be "
                    "rejected -- the cache must never mask a live safety-state change."
                )
            except (AccessError, UserError, psycopg2.errors.RaiseException) as e:
                self.assertTrue(str(e))
        finally:
            # This dict is not transaction-scoped, so it must be cleaned up explicitly --
            # this test's own record creations above roll back automatically at teardown, but
            # a stray cache entry pointing at a since-rolled-back uid would not.
            security_utils_module._SERVICE_UID_CACHE.pop(cache_key, None)

    def test_32_get_service_uid_cache_miss_resolves_live_and_does_not_pollute_the_cache(self):
        # Tests [@ANCHOR: zero_sudo:COMM_get_service_uid]
        """A service account created AFTER this worker's own startup (this test's own fresh
        fixture) is, by construction, a cache miss -- _get_service_uid() must still resolve and
        verify it correctly via the original full SQL function, and must NOT write the result
        into the process-level cache (that cache is not transaction-scoped, so a value written
        here would leak into every later test in this same worker even after this test's own
        transaction rolls back)."""
        rogue_but_valid_user = self.env["res.users"].create(
            {
                "name": "Freshly Created Service Account",
                "login": "freshly_created_service_account",
                "is_service_account": True,
            }
        )
        xml_id_name = "freshly_created_service_account_xmlid"
        self.env["ir.model.data"].create(
            {
                "module": "test_module",
                "name": xml_id_name,
                "model": "res.users",
                "res_id": rogue_but_valid_user.id,
            }
        )
        full_xml_id = f"test_module.{xml_id_name}"
        # Bug-hunt fix, 2026-09-13: same tuple-key mismatch as test_29/31's own fixes above --
        # checking a bare-string key here would always trivially pass (real entries are keyed by
        # (dbname, xml_id) since the same-day _SERVICE_UID_CACHE re-keying), silently testing
        # nothing about the real cache shape.
        cache_key = (self.env.cr.dbname, full_xml_id)
        self.assertNotIn(
            cache_key,
            security_utils_module._SERVICE_UID_CACHE,
            "Test setup assumption: a service account created mid-test must not already be in "
            "the startup-time cache, or this test cannot tell a miss apart from a hit.",
        )

        utils = self.env["zero_sudo.security.utils"]
        uid = utils._get_service_uid(full_xml_id)
        self.assertEqual(uid, rogue_but_valid_user.id)
        self.assertNotIn(
            cache_key,
            security_utils_module._SERVICE_UID_CACHE,
            "A cache miss must resolve live without writing the result back into the "
            "process-level cache -- see _get_service_uid()'s own comment on why (a value "
            "written from inside a transaction that later rolls back would otherwise leak "
            "into every later call in this same worker process).",
        )
