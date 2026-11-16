# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
import odoo.addons.distributed_redis_cache.redis_cache as rc


@tagged("post_install", "-at_install")
class TestCacheCryptoSecretRefusals(HamsTransactionCase):
    """The cache HMAC key comes only from a configured secret; a missing or public one means no key."""

    def _configure(self, env_key=None, admin_passwd=None):
        self.safe_patch_object(rc.os, "environ", {"HAMS_CRYPTO_KEY": env_key} if env_key else {})
        self.safe_patch_object(rc.os.path, "exists", return_value=False)
        self.safe_patch_object(rc.tools.config, "get", side_effect=lambda key, default=None: admin_passwd if key == "admin_passwd" else default)
        self.safe_patch_object(rc, "_warned_missing_crypto_secret", False)

    def test_environment_secret_gives_a_key(self):
        # Tests [@ANCHOR: distributed_redis_cache:COMM_raw_crypto_secret]
        # Tests [@ANCHOR: distributed_redis_cache:COMM_cache_hmac_key]
        self._configure(env_key="configured-secret")
        self.assertEqual(rc._raw_crypto_secret(), "configured-secret")
        self.assertEqual(len(rc._cache_hmac_key()), 32)

    def test_admin_password_is_the_last_resort_secret(self):
        # Tests [@ANCHOR: distributed_redis_cache:COMM_raw_crypto_secret]
        self._configure(admin_passwd="operator-chosen")
        self.assertEqual(rc._raw_crypto_secret(), "operator-chosen")

    def test_default_or_missing_secret_means_no_key_and_one_error_log(self):
        # Tests [@ANCHOR: distributed_redis_cache:COMM_raw_crypto_secret]
        # Tests [@ANCHOR: distributed_redis_cache:COMM_cache_hmac_key]
        for admin_passwd in ("admin", None, ""):
            self._configure(admin_passwd=admin_passwd)
            with self.assertLogs(rc._logger, "ERROR"):
                self.assertEqual(rc._raw_crypto_secret(), "", admin_passwd)
            self.assertIsNone(rc._cache_hmac_key(), admin_passwd)
            with self.assertNoLogs(rc._logger, "ERROR"):
                rc._raw_crypto_secret()

    def test_stored_payload_without_a_signature_is_refused(self):
        # Tests [@ANCHOR: distributed_redis_cache:COMM_verify_and_unwrap_payload]
        key = b"k" * 32
        with self.assertRaises(ValueError):
            rc._verify_and_unwrap_payload(key, rc.hmac.new(key, b"", rc.hashlib.sha256).hexdigest())
        signed = rc._sign_payload(key, b"payload")
        self.assertEqual(rc._verify_and_unwrap_payload(key, signed), b"payload")
        with self.assertRaises(ValueError):
            rc._verify_and_unwrap_payload(b"x" * 32, signed)
