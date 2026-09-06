# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0-or-later License.
import os
from cryptography.fernet import Fernet
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestBackupCrypto(HamsTransactionCase):
    """
    kopia_password/secret_key are stored encrypted (kopia_password_crypt/
    secret_key_crypt) via a real Fernet key from ODOO_BACKUP_CRYPTO_KEY or
    HAMS_CRYPTO_KEY -- nothing in this module had ever actually round-
    tripped a value through real encrypt/decrypt before this.
    """

    def setUp(self):
        super().setUp()
        self.env.user.write(
            {
                "group_ids": [
                    (4, self.env.ref("backup_management.group_backup_admin").id)
                ]
            }
        )
        self._original_env = os.environ.copy()  # burn-ignore-env
        os.environ["ODOO_BACKUP_CRYPTO_KEY"] = Fernet.generate_key().decode(  # burn-ignore-env
            "utf-8"
        )

    def tearDown(self):
        os.environ.clear()  # burn-ignore-env
        os.environ.update(self._original_env)  # burn-ignore-env
        super().tearDown()

    def test_kopia_password_round_trips_through_real_encryption(self):
        # Tests [@ANCHOR: backup_management:COMM_crypt_field]

        # Tests [@ANCHOR: backup_management:COMM_compute_encrypted_field]

        # Tests [@ANCHOR: backup_management:COMM_inverse_encrypted_field]

        # Tests [@ANCHOR: backup_management:COMM_compute_kopia_password]

        # Tests [@ANCHOR: backup_management:COMM_inverse_kopia_password]
        config = self.env["backup.config"].create(
            {
                "name": "Crypto Test Kopia",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/crypto_test_kopia",
                "kopia_password": "s3cr3t-passphrase",
            }
        )
        self.assertTrue(config.kopia_password_crypt)
        self.assertNotEqual(config.kopia_password_crypt, "s3cr3t-passphrase")
        config.invalidate_recordset(["kopia_password"])
        self.assertEqual(config.kopia_password, "s3cr3t-passphrase")

    def test_secret_key_round_trips_through_real_encryption(self):
        # Tests [@ANCHOR: backup_management:COMM_compute_secret_key]

        # Tests [@ANCHOR: backup_management:COMM_inverse_secret_key]
        config = self.env["backup.config"].create(
            {
                "name": "Crypto Test S3",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/crypto_test_s3",
                "storage_type": "s3",
                "secret_key": "AKIA_FAKE_SECRET_VALUE",
            }
        )
        self.assertTrue(config.secret_key_crypt)
        self.assertNotEqual(config.secret_key_crypt, "AKIA_FAKE_SECRET_VALUE")
        config.invalidate_recordset(["secret_key"])
        self.assertEqual(config.secret_key, "AKIA_FAKE_SECRET_VALUE")

    def test_no_crypto_key_configured_leaves_crypt_field_falsy(self):
        del os.environ["ODOO_BACKUP_CRYPTO_KEY"]  # burn-ignore-env
        config = self.env["backup.config"].create(
            {
                "name": "No Key Test",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/no_key_test",
                "kopia_password": "whatever",
            }
        )
        self.assertFalse(config.kopia_password_crypt)
