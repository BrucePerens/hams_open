# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Credential and engine refusals of backup.config that no other test reached: what _crypt_field does with no key,
an empty value, or a ciphertext the key cannot open; an engine that is missing or unknown; a configuration with no
job; and the minimum-size alarm on newly ingested snapshots. Only in-process environment values are used (restored
after each test); no real backup key is read, written or changed."""
import os

from cryptography.fernet import Fernet
from odoo.exceptions import UserError
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

CONFIG = "odoo.addons.backup_management.models.backup_config."


@tagged("post_install", "-at_install")
class TestBackupConfigRefusals(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        saved = os.environ.copy()
        self.addCleanup(lambda: (os.environ.clear(), os.environ.update(saved)))
        self.Config = self.env["backup.config"]

    def _no_key(self):
        os.environ.pop("ODOO_BACKUP_CRYPTO_KEY", None)
        os.environ.pop("HAMS_CRYPTO_KEY", None)

    def _config(self, **extra):
        vals = {"name": "Refusal Config", "engine": "kopia", "target_path": "/var/lib/odoo/backups/refusal_config"}
        vals.update(extra)
        return self.Config.create(vals)

    def test_01_with_no_key_nothing_is_decrypted_empty_values_are_not_an_error_and_a_secret_is_refused(self):
        # Tests [@ANCHOR: backup_management:get_fernet]
        # Tests [@ANCHOR: backup_management:COMM_crypt_field]
        self._no_key()
        self.assertIsNone(self.Config._get_fernet())
        self.assertFalse(self.Config._crypt_field("gAAAAAB-anything", decrypt=True))
        self.assertFalse(self.Config._crypt_field(False))
        with self.assertRaisesRegex(UserError, "no backup encryption key"):
            self.Config._crypt_field("typed-secret")

    def test_02_a_ciphertext_the_key_cannot_open_reads_as_the_error_placeholder_never_as_plaintext(self):
        # Tests [@ANCHOR: backup_management:COMM_crypt_field]
        os.environ["ODOO_BACKUP_CRYPTO_KEY"] = Fernet.generate_key().decode("utf-8")
        foreign = Fernet(Fernet.generate_key()).encrypt(b"secret").decode("utf-8")
        self.assertEqual(self.Config._crypt_field(foreign, decrypt=True), "***ERROR***")
        self.assertEqual(self.Config._crypt_field("not-base64-at-all!", decrypt=True), "***ERROR***")
        self.assertFalse(self.Config._crypt_field("", decrypt=True))

    def test_03_a_missing_pgbackrest_or_an_unknown_engine_is_refused(self):
        # Tests [@ANCHOR: backup_management:get_executable]
        self.safe_patch(CONFIG + "shutil.which").return_value = None
        with self.assertRaisesRegex(UserError, "pgBackRest is missing"):
            self.Config._get_executable("pgbackrest")
        with self.assertRaisesRegex(UserError, "Unknown engine"):
            self.Config._get_executable("rsync")

    def test_04_view_latest_job_with_no_job_is_refused(self):
        # Tests [@ANCHOR: backup_management:COMM_action_view_latest_job]
        config = self._config()
        self.env["backup.job"].search([("config_id", "=", config.id)]).unlink()
        with self.assertRaisesRegex(UserError, "No jobs found"):
            config.action_view_latest_job()

    def _kopia(self, sid, size_bytes, failed=0):
        return {
            "id": sid, "startTime": "2026-11-15T10:00:00Z",
            "rootEntry": {"summ": {"size": size_bytes, "numFailed": failed}},
        }

    def test_05_a_new_snapshot_below_the_minimum_size_is_reported_once_and_a_large_one_is_not(self):
        # Tests [@ANCHOR: backup_management:COMM_upsert_snapshots_roundtrip_optimization]
        config = self._config(minimum_size_mb=10)
        report = self.safe_patch_object(type(config), "report_backup_failure")
        config._process_snapshot_data([self._kopia("snap_small", 1024), self._kopia("snap_big", 50 * 1024 * 1024)], "kopia")
        self.assertEqual(report.call_count, 1)
        self.assertIn("snap_small", report.call_args.args[0])
        self.assertIn("below the 10 MB minimum", report.call_args.args[0])
        config._process_snapshot_data([self._kopia("snap_small", 1024)], "kopia")
        self.assertEqual(report.call_count, 1, "an already known snapshot must not be reported again")

    def test_06_no_minimum_means_no_size_alarm_and_a_snapshot_with_failed_files_is_partial(self):
        # Tests [@ANCHOR: backup_management:COMM_upsert_snapshots_roundtrip_optimization]
        config = self._config(minimum_size_mb=0)
        report = self.safe_patch_object(type(config), "report_backup_failure")
        config._process_snapshot_data([self._kopia("snap_partial", 1, failed=3)], "kopia")
        report.assert_not_called()
        snap = self.env["backup.snapshot"].search([("config_id", "=", config.id), ("snapshot_id", "=", "snap_partial")])
        self.assertEqual(snap.status, "partial")

    def test_07_a_pgbackrest_start_time_outside_the_supported_years_is_no_time_not_a_crash(self):
        # Tests [@ANCHOR: backup_management:COMM_upsert_snapshots_roundtrip_optimization]
        config = self._config(engine="pgbackrest", target_path="refusalstanza")
        data = [{"backup": [{"label": "20261115-100000F", "timestamp": {"start": 10 ** 30}, "info": {"size": 5}}]}]
        config._process_snapshot_data(data, "pgbackrest")
        snap = self.env["backup.snapshot"].search([("config_id", "=", config.id), ("snapshot_id", "=", "20261115-100000F")])
        self.assertEqual(len(snap), 1)
        self.assertFalse(snap.start_time)

    def test_publish_refuses_a_credential_that_reads_as_the_error_placeholder(self):
        # Tests [@ANCHOR: backup_management:COMM_publish_to_worker]
        os.environ["ODOO_BACKUP_CRYPTO_KEY"] = Fernet.generate_key().decode("utf-8")
        config = self._config(secret_key="s3-secret", kopia_password="kopia-pw")
        config.flush_recordset()
        os.environ["ODOO_BACKUP_CRYPTO_KEY"] = Fernet.generate_key().decode("utf-8")
        config.invalidate_recordset()
        self.assertEqual(config.kopia_password, "***ERROR***")
        jobs_before = self.env["backup.job"].search_count([("config_id", "=", config.id)])
        postcommit_before = len(self.env.cr.postcommit._funcs)
        with self.assertRaisesRegex(UserError, "cannot be decrypted") as ctx:
            config._publish_to_worker("kopia")
        self.assertNotIn("***ERROR***", str(ctx.exception))
        self.assertEqual(self.env["backup.job"].search_count([("config_id", "=", config.id)]), jobs_before)
        self.assertEqual(len(self.env.cr.postcommit._funcs), postcommit_before)
