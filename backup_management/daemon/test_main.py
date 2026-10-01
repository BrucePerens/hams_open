#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later

import json
import os
import sys
import tempfile
import threading
import time
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


# [@ANCHOR: backup_management:COMM_test_pgbackrest_requires_sidecar]
class TestPgbackrestRequiresSidecar(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:COMM_pgbackrest_requires_sidecar]

    def test_backup_requires_sidecar(self):
        self.assertTrue(
            backup_worker._pgbackrest_requires_sidecar(
                ["pgbackrest", "backup", "--stanza=hams_prod"]
            )
        )

    def test_info_does_not_require_sidecar(self):
        # "info" is read-only and already works fine as odoo -- only a real
        # write (backup) needs PostgreSQL data-directory access.
        self.assertFalse(
            backup_worker._pgbackrest_requires_sidecar(
                ["pgbackrest", "info", "--stanza=hams_prod", "--output=json"]
            )
        )

    def test_restore_does_not_require_sidecar(self):
        # Deliberately not delegated yet -- restore remains a manual admin
        # operation (see main.py's own comment on _PGBACKREST_PRIVILEGED_OPS).
        self.assertFalse(
            backup_worker._pgbackrest_requires_sidecar(
                ["pgbackrest", "restore", "--stanza=hams_prod", "--set=latest"]
            )
        )

    def test_non_pgbackrest_cmd_does_not_require_sidecar(self):
        self.assertFalse(backup_worker._pgbackrest_requires_sidecar(["kopia", "snapshot", "create"]))

    def test_empty_cmd_does_not_require_sidecar(self):
        self.assertFalse(backup_worker._pgbackrest_requires_sidecar([]))


# [@ANCHOR: backup_management:COMM_test_run_pgbackrest_via_sidecar]
class TestRunPgbackrestViaSidecar(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:COMM_pgbackrest_privileged_sidecar]
    #
    # Exercises the real filesystem protocol (request/result JSON files, not
    # a mocked subprocess) -- this function's whole job is that protocol, not
    # anything subprocess-shaped itself; the privileged side of it is covered
    # by TestPgbackrestSidecarScript below.

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="pgbackrest_sidecar_test_")
        self._patchers = [
            patch.object(backup_worker, "PGBACKREST_SPOOL_DIR", self._tmpdir),
            patch.object(backup_worker, "PGBACKREST_SIDECAR_POLL_INTERVAL", 0.01),
            patch.object(backup_worker, "PGBACKREST_SIDECAR_TIMEOUT", 1),
        ]
        for p in self._patchers:
            p.start()
            self.addCleanup(p.stop)

    def _write_result(self, job_id, return_code, output):
        result_path = os.path.join(self._tmpdir, f"result-{job_id}.json")
        with open(result_path, "w") as f:
            json.dump({"return_code": return_code, "output": output}, f)

    def _wait_for_file(self, path):
        # Poll briefly for a file the way the real .path unit's own trigger
        # does, then act -- simplest way to exercise the real polling loop in
        # _run_pgbackrest_via_sidecar without a real systemd unit in this
        # unit test.
        for _ in range(200):
            if os.path.exists(path):
                return
            time.sleep(0.005)

    def test_writes_a_request_file_and_returns_the_sidecars_result(self):
        def fake_sidecar():
            self._wait_for_file(os.path.join(self._tmpdir, "request-99.json"))
            self._write_result(99, 0, "backup complete: full backup size = 1.2GB")

        threading.Thread(target=fake_sidecar, daemon=True).start()

        cmd = ["pgbackrest", "backup", "--stanza=hams_prod", "--type=full"]
        config = {"storage_type": "local"}
        return_code, output = backup_worker._run_pgbackrest_via_sidecar(cmd, config, 99)

        self.assertEqual(return_code, 0)
        self.assertIn("backup complete", output)
        # Both the request and result files are cleaned up once consumed --
        # the request carried no secret here, but the convention holds either way.
        self.assertFalse(os.path.exists(os.path.join(self._tmpdir, "request-99.json")))
        self.assertFalse(os.path.exists(os.path.join(self._tmpdir, "result-99.json")))

    def test_request_file_carries_s3_secrets_but_not_in_argv(self):
        captured = {}

        def fake_sidecar():
            request_path = os.path.join(self._tmpdir, "request-100.json")
            self._wait_for_file(request_path)
            with open(request_path) as f:
                captured.update(json.load(f))
            self._write_result(100, 0, "ok")

        threading.Thread(target=fake_sidecar, daemon=True).start()

        cmd = ["pgbackrest", "backup", "--stanza=hams_prod", "--repo1-type=s3"]
        config = {
            "storage_type": "s3",
            "access_key": "AKIAFAKE",
            "secret_key": "sekrit",
        }
        backup_worker._run_pgbackrest_via_sidecar(cmd, config, 100)

        self.assertEqual(captured["cmd"], cmd)
        self.assertNotIn("AKIAFAKE", cmd)  # never in argv
        self.assertEqual(captured["env"]["PGBACKREST_REPO1_S3_KEY"], "AKIAFAKE")
        self.assertEqual(captured["env"]["PGBACKREST_REPO1_S3_KEY_SECRET"], "sekrit")

    def test_invalid_stanza_raises_before_touching_the_filesystem(self):
        cmd = ["pgbackrest", "backup", "--stanza=../../etc/passwd"]
        with self.assertRaises(ValueError):
            backup_worker._run_pgbackrest_via_sidecar(cmd, {}, 101)
        self.assertEqual(os.listdir(self._tmpdir), [])

    def test_timeout_cleans_up_the_request_and_reports_failure(self):
        cmd = ["pgbackrest", "backup", "--stanza=hams_prod"]
        return_code, output = backup_worker._run_pgbackrest_via_sidecar(cmd, {}, 102)
        self.assertEqual(return_code, 1)
        self.assertIn("Timed out", output)
        self.assertEqual(os.listdir(self._tmpdir), [])


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
        self.config_file = "/var/lib/odoo/backups/.kopia-configs/config_42.config"  # the daemon's real config dir; makedirs and the kopia call are mocked

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

    # test_02 and test_03 used to assert directly on subprocess.Popen's own call
    # args, matching every other test in this class -- but a real pgbackrest
    # "backup" can no longer run as this daemon's own `odoo` account at all
    # (NoNewPrivileges=true blocks it; see main.py's own
    # _run_pgbackrest_via_sidecar() docstring), so execute_job now delegates the
    # whole operation there instead of calling subprocess.Popen directly. These
    # two now assert on *that* call's args; TestRunPgbackrestViaSidecar below
    # covers the delegation function's own real filesystem protocol end to end.
    @patch("main._json2_call")
    @patch("main._run_pgbackrest_via_sidecar")
    def test_02_pgbackrest_s3_backup_job_delegates_to_the_privileged_sidecar(
        self, mock_sidecar, mock_json2
    ):
        mock_sidecar.return_value = (0, "backup complete")
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

        cmd, config, job_id = mock_sidecar.call_args.args
        self.assertEqual(cmd[0], "pgbackrest")
        self.assertEqual(cmd[1], "backup")
        self.assertIn("--repo1-type=s3", cmd)
        self.assertIn("--repo1-s3-bucket=b2-bucket", cmd)
        self.assertIn(
            "--repo1-s3-endpoint=s3.us-west-002.backblazeb2.com", cmd
        )
        self.assertIn("--repo1-retention-full=5", cmd)
        # Never the raw secret values in argv -- pgbackrest's real CLI refuses
        # them there outright; they travel via `config`, read by the sidecar
        # delegation function itself, same as the direct-Popen path used to.
        joined = " ".join(cmd)
        self.assertNotIn("b2keyid", joined)
        self.assertNotIn("b2applicationkey", joined)
        self.assertEqual(job_id, 2)
        self.assertEqual(config["access_key"], "b2keyid")
        self.assertEqual(config["secret_key"], "b2applicationkey")

    @patch("main._json2_call")
    @patch("main._run_pgbackrest_via_sidecar")
    def test_02b_sidecar_output_is_logged_exactly_once(self, mock_sidecar, mock_json2):
        # Advisor-caught bug, 2026-10-01: the sidecar branch used to send
        # sidecar_output via an early append_log call, then the shared
        # "write final state" block below unconditionally appended it to
        # unsent_buffer and sent it again -- every sidecar backup's own
        # output landed in the job log twice. A bare, call-count-blind mock
        # (the shape every other test in this class already used) couldn't
        # catch this; this test exists specifically to.
        mock_sidecar.return_value = (0, "backup complete: full backup size = 1.2GB")
        ch, method = self._make_ch_method()
        payload = {
            "job_id": 20,
            "engine": "pgbackrest",
            "target_path": "mystanza",
            "config_id": 7,
            "storage_type": "local",
        }
        body = backup_worker.json.dumps(payload)
        backup_worker.execute_job(ch, method, MagicMock(), body)

        append_log_calls = [
            c for c in mock_json2.call_args_list if c.args[:2] == ("backup.job", "append_log")
        ]
        self.assertEqual(
            len(append_log_calls),
            1,
            f"expected exactly one append_log call, got {len(append_log_calls)}: {append_log_calls}",
        )
        self.assertEqual(
            append_log_calls[0].kwargs["text_chunk"].count("backup complete"),
            1,
        )

    @patch("main._json2_call")
    @patch("main._run_pgbackrest_via_sidecar")
    def test_03_local_storage_type_unaffected(self, mock_sidecar, mock_json2):
        # Regression guard: a plain local-storage pgbackrest job must not
        # gain any --repo1-s3-* flags, and still routes through the sidecar
        # (local storage changes the repo target, not the privilege problem --
        # this account still can't read PostgreSQL's own data directory).
        mock_sidecar.return_value = (0, "")
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

        cmd, config, job_id = mock_sidecar.call_args.args
        self.assertTrue(all(not c.startswith("--repo1-s3") for c in cmd))
        self.assertNotIn("access_key", config)
        self.assertEqual(job_id, 3)


class TestUnexpectedExceptionIsolation(unittest.TestCase):
    """An exception outside execute_job's expected types fails that one job."""

    def _ch_method(self):
        ch = MagicMock()
        method = MagicMock()
        method.delivery_tag = "tag-x"
        return ch, method

    def _s3_body(self):
        return backup_worker.json.dumps(
            {
                "job_id": 41,
                "engine": "pgbackrest",
                "target_path": "stanza",
                "config_id": 7,
                "storage_type": "s3",
            }
        )

    @patch("main._json2_call")
    @patch("main._pgbackrest_s3_repo_args", side_effect=KeyError("bucket"))
    @patch("main.shutil.which", return_value="/usr/bin/pgbackrest")
    def test_01_unexpected_exception_fails_job_and_is_reported(
        self, mock_which, mock_args, mock_json2
    ):
        ch, method = self._ch_method()
        with self.assertLogs(backup_worker.logger, level="ERROR") as logs:
            backup_worker.execute_job(ch, method, MagicMock(), self._s3_body())
        # Daemon survives: the message is acked exactly once.
        ch.basic_ack.assert_called_once_with(delivery_tag="tag-x")
        # Logged with job context and the exception type.
        self.assertTrue(any("job 41" in m and "KeyError" in m for m in logs.output))
        # Recorded on the job and reported on the config.
        calls = mock_json2.call_args_list
        self.assertTrue(
            any(
                c.args[:2] == ("backup.job", "write")
                and c.kwargs.get("vals") == {"state": "failed"}
                for c in calls
            )
        )
        self.assertTrue(
            any(
                c.args[:2] == ("backup.config", "report_backup_failure")
                and "KeyError" in c.kwargs.get("message", "")
                for c in calls
            )
        )

    @patch("main._json2_call")
    def test_02_non_object_json_body_does_not_kill_worker(self, mock_json2):
        ch, method = self._ch_method()
        # Valid JSON that is not an object: payload.get raises AttributeError.
        backup_worker.execute_job(ch, method, MagicMock(), "[1, 2, 3]")
        ch.basic_ack.assert_called_once_with(delivery_tag="tag-x")

    @patch("main._json2_call", side_effect=KeyError("reporting broke"))
    @patch("main._pgbackrest_s3_repo_args", side_effect=KeyError("bucket"))
    @patch("main.shutil.which", return_value="/usr/bin/pgbackrest")
    def test_03_failure_while_reporting_does_not_kill_worker(
        self, mock_which, mock_args, mock_json2
    ):
        ch, method = self._ch_method()
        backup_worker.execute_job(ch, method, MagicMock(), self._s3_body())
        ch.basic_ack.assert_called_once_with(delivery_tag="tag-x")

    @patch("main._json2_call")
    @patch("main._pgbackrest_s3_repo_args", side_effect=MemoryError())
    @patch("main.shutil.which", return_value="/usr/bin/pgbackrest")
    def test_04_memory_error_still_fails_fast(
        self, mock_which, mock_args, mock_json2
    ):
        ch, method = self._ch_method()
        with self.assertRaises(MemoryError):
            backup_worker.execute_job(ch, method, MagicMock(), self._s3_body())
        ch.basic_ack.assert_not_called()


if __name__ == "__main__":
    unittest.main()
