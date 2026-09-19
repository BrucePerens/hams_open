#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later

import os
import sys
import unittest
from unittest.mock import MagicMock, patch

sys.path.append(os.path.dirname(os.path.abspath(__file__)))
import main as backup_worker  # noqa: E402


class TestRabbitMQCredentialFailFast(unittest.TestCase):
    def test_01_missing_both_raises(self):
        with patch.object(backup_worker, "RMQ_USER", None), patch.object(
            backup_worker, "RMQ_PASS", None
        ):
            with self.assertRaises(RuntimeError):
                backup_worker._require_rabbitmq_credentials()

    def test_02_missing_user_only_raises(self):
        with patch.object(backup_worker, "RMQ_USER", None), patch.object(
            backup_worker, "RMQ_PASS", "somepass"
        ):
            with self.assertRaises(RuntimeError):
                backup_worker._require_rabbitmq_credentials()

    def test_03_missing_pass_only_raises(self):
        with patch.object(backup_worker, "RMQ_USER", "someuser"), patch.object(
            backup_worker, "RMQ_PASS", None
        ):
            with self.assertRaises(RuntimeError):
                backup_worker._require_rabbitmq_credentials()

    def test_04_never_falls_back_to_guest_guest(self):
        # The whole point of the check: even the literal string "guest"
        # for both must pass through unmodified if explicitly set -- this
        # test only asserts the function doesn't itself substitute a
        # default; it does not endorse guest/guest as a real credential.
        with patch.object(backup_worker, "RMQ_USER", "guest"), patch.object(
            backup_worker, "RMQ_PASS", "guest"
        ):
            backup_worker._require_rabbitmq_credentials()  # must not raise

    def test_05_both_set_does_not_raise(self):
        with patch.object(backup_worker, "RMQ_USER", "real_user"), patch.object(
            backup_worker, "RMQ_PASS", "real_pass"
        ):
            backup_worker._require_rabbitmq_credentials()  # must not raise


# [@ANCHOR: backup_management:COMM_test_strip_endpoint_scheme]
class TestStripEndpointScheme(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:COMM_strip_endpoint_scheme]

    def test_01_empty_endpoint(self):
        self.assertEqual(backup_worker._strip_endpoint_scheme(""), ("", False))
        self.assertEqual(backup_worker._strip_endpoint_scheme(None), ("", False))

    def test_02_https_scheme_stripped_tls_stays_enabled(self):
        host, disable_tls = backup_worker._strip_endpoint_scheme(
            "https://s3.us-west-002.backblazeb2.com"
        )
        self.assertEqual(host, "s3.us-west-002.backblazeb2.com")
        self.assertFalse(disable_tls)

    def test_03_http_scheme_stripped_tls_disabled(self):
        host, disable_tls = backup_worker._strip_endpoint_scheme(
            "http://minio.internal:9000"
        )
        self.assertEqual(host, "minio.internal:9000")
        self.assertTrue(disable_tls)

    def test_04_bare_host_passes_through(self):
        host, disable_tls = backup_worker._strip_endpoint_scheme("s3.amazonaws.com")
        self.assertEqual(host, "s3.amazonaws.com")
        self.assertFalse(disable_tls)

    def test_05_trailing_slash_stripped(self):
        host, _ = backup_worker._strip_endpoint_scheme("https://example.com/")
        self.assertEqual(host, "example.com")


# [@ANCHOR: backup_management:COMM_test_pgbackrest_s3_repo_args]
class TestPgbackrestS3RepoArgs(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:COMM_pgbackrest_s3_repo_args]

    def test_01_builds_real_repo1_flags_no_secrets(self):
        config = {
            "storage_type": "s3",
            "bucket_name": "my-bucket",
            "endpoint_url": "https://s3.amazonaws.com",
            "access_key": "AKIASECRETLOOKING",
            "secret_key": "topsecretvalue",
        }
        args = backup_worker._pgbackrest_s3_repo_args(config, "mystanza")
        self.assertIn("--repo1-type=s3", args)
        self.assertIn("--repo1-s3-bucket=my-bucket", args)
        self.assertIn("--repo1-s3-endpoint=s3.amazonaws.com", args)
        self.assertIn("--repo1-s3-region=us-east-1", args)
        self.assertIn("--repo1-path=/mystanza", args)
        # The whole point: access_key/secret_key must never appear in argv --
        # pgbackrest's real CLI refuses them there outright, and this is also
        # what keeps them out of main.py's own "Executing: %s" argv log line.
        joined = " ".join(args)
        self.assertNotIn("AKIASECRETLOOKING", joined)
        self.assertNotIn("topsecretvalue", joined)
        self.assertNotIn("--repo1-s3-key", joined)

    def test_02_b2_uses_s3_compatible_endpoint(self):
        config = {
            "storage_type": "b2",
            "bucket_name": "b2-bucket",
            "endpoint_url": "https://s3.us-west-002.backblazeb2.com",
        }
        args = backup_worker._pgbackrest_s3_repo_args(config, "b2stanza")
        self.assertIn("--repo1-type=s3", args)
        self.assertIn(
            "--repo1-s3-endpoint=s3.us-west-002.backblazeb2.com", args
        )

    def test_03_region_override_honored(self):
        config = {"bucket_name": "b", "region": "eu-central-1"}
        args = backup_worker._pgbackrest_s3_repo_args(config, "stanza")
        self.assertIn("--repo1-s3-region=eu-central-1", args)

    def test_04_missing_endpoint_omits_flag(self):
        config = {"bucket_name": "b"}
        args = backup_worker._pgbackrest_s3_repo_args(config, "stanza")
        self.assertTrue(all(not a.startswith("--repo1-s3-endpoint") for a in args))


# [@ANCHOR: backup_management:COMM_test_ensure_kopia_s3_repository]
class TestEnsureKopiaS3Repository(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:COMM_ensure_kopia_s3_repository]
    #
    # Mocks subprocess.run rather than invoking a real kopia binary/bucket
    # (no real S3/B2 credentials exist in this environment), but the
    # connect-fails-then-create / create-fails-when-already-connected
    # contract these mocks encode was verified directly against a real,
    # locally installed kopia 0.23.1 binary (filesystem backend) before
    # writing this code -- see _ensure_kopia_s3_repository's own docstring.

    def setUp(self):
        self.config = {
            "storage_type": "s3",
            "bucket_name": "my-bucket",
            "endpoint_url": "https://s3.us-west-002.backblazeb2.com",
        }
        self.env_vars = {"AWS_ACCESS_KEY_ID": "ak", "AWS_SECRET_ACCESS_KEY": "sk"}
        self.config_file = "/tmp/does-not-need-to-exist/config_42.config"

    @patch("main.os.makedirs")
    @patch("main.subprocess.run")
    def test_01_already_connected_no_create_attempted(self, mock_run, mock_makedirs):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        backup_worker._ensure_kopia_s3_repository(
            self.config, self.env_vars, self.config_file
        )
        self.assertEqual(mock_run.call_count, 1)
        connect_cmd = mock_run.call_args_list[0].args[0]
        self.assertEqual(
            connect_cmd,
            [
                "kopia",
                "repository",
                "connect",
                "s3",
                "--bucket",
                "my-bucket",
                "--endpoint",
                "s3.us-west-002.backblazeb2.com",
            ],
        )
        self.assertEqual(mock_run.call_args_list[0].kwargs["env"], self.env_vars)

    @patch("main.os.makedirs")
    @patch("main.subprocess.run")
    def test_02_first_run_connect_fails_then_create_succeeds(
        self, mock_run, mock_makedirs
    ):
        mock_run.side_effect = [
            MagicMock(
                returncode=1,
                stdout="",
                stderr="error connecting to repository: repository not "
                "initialized in the provided storage",
            ),
            MagicMock(returncode=0, stdout="", stderr=""),
        ]
        backup_worker._ensure_kopia_s3_repository(
            self.config, self.env_vars, self.config_file
        )
        self.assertEqual(mock_run.call_count, 2)
        connect_cmd = mock_run.call_args_list[0].args[0]
        create_cmd = mock_run.call_args_list[1].args[0]
        self.assertEqual(connect_cmd[:4], ["kopia", "repository", "connect", "s3"])
        self.assertEqual(create_cmd[:4], ["kopia", "repository", "create", "s3"])
        self.assertEqual(create_cmd[4:], connect_cmd[4:])

    @patch("main.os.makedirs")
    @patch("main.subprocess.run")
    def test_03_both_connect_and_create_fail_raises_value_error(
        self, mock_run, mock_makedirs
    ):
        mock_run.side_effect = [
            MagicMock(returncode=1, stdout="", stderr="connect: access denied"),
            MagicMock(returncode=1, stdout="", stderr="create: access denied"),
        ]
        with self.assertRaises(ValueError):
            backup_worker._ensure_kopia_s3_repository(
                self.config, self.env_vars, self.config_file
            )

    @patch("main.os.makedirs")
    @patch("main.subprocess.run")
    def test_04_http_endpoint_adds_disable_tls(self, mock_run, mock_makedirs):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        http_config = dict(self.config, endpoint_url="http://minio.local:9000")
        backup_worker._ensure_kopia_s3_repository(
            http_config, self.env_vars, self.config_file
        )
        connect_cmd = mock_run.call_args_list[0].args[0]
        self.assertIn("--disable-tls", connect_cmd)
        self.assertIn("minio.local:9000", connect_cmd)


# [@ANCHOR: backup_management:COMM_test_execute_job_s3_b2]
class TestExecuteJobS3B2CommandBuilding(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:COMM_test_backup_worker_real]
    #
    # execute_job's own real, end-to-end behavior against a live RabbitMQ +
    # daemon process is covered by
    # backup_management/tests/test_backup_worker_real.py
    # (RealTransactionCase, no real S3/B2 credentials required there
    # either -- it exercises the "dummy"/unrecognized-engine path). These
    # tests mirror this file's own existing convention instead
    # (unittest.mock over a real subprocess), asserting on the exact argv
    # and env dict execute_job hands to subprocess.Popen/subprocess.run
    # for the new S3/B2 code paths specifically.

    def _make_ch_method(self):
        ch = MagicMock()
        method = MagicMock()
        method.delivery_tag = "tag-1"
        return ch, method

    def _popen_result(self, stdout_text=""):
        proc = MagicMock()
        # execute_job reads stdout in a loop until read() returns falsy.
        proc.stdout.read.side_effect = [stdout_text, ""]
        proc.wait.return_value = 0
        return proc

    @patch("main._json2_call")
    @patch("main.subprocess.Popen")
    @patch("main.subprocess.run")
    @patch("main.shutil.which", return_value="/usr/bin/kopia")
    @patch("main.os.makedirs")
    def test_01_kopia_s3_job_connects_before_snapshot_and_hides_secrets(
        self, mock_makedirs, mock_which, mock_run, mock_popen, mock_json2
    ):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        mock_popen.return_value = self._popen_result()
        ch, method = self._make_ch_method()
        payload = {
            "job_id": 1,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/site1",
            "config_id": 42,
            "storage_type": "s3",
            "bucket_name": "hams-backups",
            "endpoint_url": "https://s3.amazonaws.com",
            "access_key": "AKIAFAKEKEY",
            "secret_key": "fakesecretvalue",
        }
        body = backup_worker.json.dumps(payload)
        backup_worker.execute_job(ch, method, MagicMock(), body)

        # The pre-step: kopia repository connect ran before the snapshot cmd.
        connect_cmd = mock_run.call_args_list[0].args[0]
        self.assertEqual(connect_cmd[:4], ["kopia", "repository", "connect", "s3"])
        self.assertIn("hams-backups", connect_cmd)

        # The main snapshot command itself is unchanged -- no S3 flags belong
        # on it; the repository is already selected via KOPIA_CONFIG_PATH.
        snapshot_cmd = mock_popen.call_args.args[0]
        self.assertEqual(
            snapshot_cmd,
            [
                "kopia",
                "snapshot",
                "create",
                "--json",
                "--",
                "/var/lib/odoo/backups/site1",
            ],
        )

        popen_env = mock_popen.call_args.kwargs["env"]
        self.assertEqual(popen_env["AWS_ACCESS_KEY_ID"], "AKIAFAKEKEY")
        self.assertEqual(popen_env["AWS_SECRET_ACCESS_KEY"], "fakesecretvalue")
        self.assertEqual(
            popen_env["KOPIA_CONFIG_PATH"],
            backup_worker._kopia_config_file(42),
        )

        run_env = mock_run.call_args_list[0].kwargs["env"]
        self.assertEqual(run_env["AWS_ACCESS_KEY_ID"], "AKIAFAKEKEY")

    @patch("main._json2_call")
    @patch("main.subprocess.Popen")
    @patch("main.shutil.which", return_value="/usr/bin/pgbackrest")
    def test_02_pgbackrest_s3_job_command_and_env(
        self, mock_which, mock_popen, mock_json2
    ):
        mock_popen.return_value = self._popen_result()
        ch, method = self._make_ch_method()
        payload = {
            "job_id": 2,
            "engine": "pgbackrest",
            "target_path": "mystanza",
            "config_id": 7,
            "storage_type": "b2",
            "bucket_name": "b2-bucket",
            "endpoint_url": "https://s3.us-west-002.backblazeb2.com",
            "access_key": "b2keyid",
            "secret_key": "b2applicationkey",
            "keep_daily": 5,
        }
        body = backup_worker.json.dumps(payload)
        backup_worker.execute_job(ch, method, MagicMock(), body)

        cmd = mock_popen.call_args.args[0]
        self.assertEqual(cmd[0], "pgbackrest")
        self.assertIn("--repo1-type=s3", cmd)
        self.assertIn("--repo1-s3-bucket=b2-bucket", cmd)
        self.assertIn(
            "--repo1-s3-endpoint=s3.us-west-002.backblazeb2.com", cmd
        )
        self.assertIn("--repo1-retention-full=5", cmd)
        # Never the raw secret flags -- pgbackrest's real CLI refuses them.
        joined = " ".join(cmd)
        self.assertNotIn("b2keyid", joined)
        self.assertNotIn("b2applicationkey", joined)

        env = mock_popen.call_args.kwargs["env"]
        self.assertEqual(env["PGBACKREST_REPO1_S3_KEY"], "b2keyid")
        self.assertEqual(env["PGBACKREST_REPO1_S3_KEY_SECRET"], "b2applicationkey")

    @patch("main._json2_call")
    @patch("main.subprocess.Popen")
    @patch("main.shutil.which", return_value="/usr/bin/pgbackrest")
    def test_03_local_storage_type_unaffected(self, mock_which, mock_popen, mock_json2):
        # Regression guard: a plain local-storage pgbackrest job must not
        # gain any --repo1-s3-* flags or PGBACKREST_REPO1_S3_* env vars.
        mock_popen.return_value = self._popen_result()
        ch, method = self._make_ch_method()
        payload = {
            "job_id": 3,
            "engine": "pgbackrest",
            "target_path": "localstanza",
            "config_id": 9,
            "storage_type": "local",
        }
        body = backup_worker.json.dumps(payload)
        backup_worker.execute_job(ch, method, MagicMock(), body)

        cmd = mock_popen.call_args.args[0]
        self.assertTrue(all(not c.startswith("--repo1-s3") for c in cmd))
        env = mock_popen.call_args.kwargs["env"]
        self.assertNotIn("PGBACKREST_REPO1_S3_KEY", env)


if __name__ == "__main__":
    unittest.main()
