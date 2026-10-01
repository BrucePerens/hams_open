# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0-or-later License.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError
import psycopg2
from odoo.tools import mute_logger
import json


@tagged("post_install", "-at_install")
class TestBatch2Fixes(HamsTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.env.user.group_ids |= cls.env.ref("backup_management.group_backup_admin")
        cls.config1 = cls.env["backup.config"].create({
            "name": "Config 1",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_kopia1",
            "storage_type": "local",
        })
        cls.config_pg = cls.env["backup.config"].create({
            "name": "Config PG",
            "engine": "pgbackrest",
            "target_path": "my_stanza",
            "storage_type": "local",
        })

    def test_sql_constraints_config(self):
        # We need to verify that we cannot create duplicate names
        # In Odoo, _sql_constraints raises IntegrityError via psycopg2
        with self.assertRaises(psycopg2.IntegrityError), mute_logger('odoo.sql_db'):
            with self.env.cr.savepoint():
                self.env["backup.config"].create({
                    "name": "Config 1", # Duplicate
                    "engine": "kopia",
                    "target_path": "/var/lib/odoo/backups/dup",
                    "storage_type": "local",
                })
                self.env.flush_all()

    def test_sql_constraints_snapshot(self):
        self.env["backup.snapshot"].create({
            "config_id": self.config1.id,
            "snapshot_id": "snap123",
        })
        with self.assertRaises(psycopg2.IntegrityError), mute_logger('odoo.sql_db'):
            with self.env.cr.savepoint():
                self.env["backup.snapshot"].create({
                    "config_id": self.config1.id,
                    "snapshot_id": "snap123", # Duplicate for same config
                })
                self.env.flush_all()

    def test_upsert_crash_empty_string(self):
        # Mock payload with empty start time
        data_kopia = [{
            "id": "snap_empty",
            "startTime": "",
            "summary": {"totalBytes": 100}
        }]
        
        # This shouldn't crash
        self.config1._process_snapshot_data(data_kopia, "kopia")

        snap = self.env["backup.snapshot"].search([("snapshot_id", "=", "snap_empty")])
        self.assertEqual(len(snap), 1)
        # Real bug found live, 2026-10-01: upsert_backup_snapshots()'s own raw SQL INSERT
        # never set `name` at all (it's a Python-level default= on the model, which a direct
        # SQL insert never goes through) -- every snapshot this procedure ever wrote read
        # back name/display_name as False. Fixed to reuse snapshot_id as name.
        self.assertEqual(snap.name, "snap_empty")

    def test_upsert_crash_false(self):
        data_pg = [{
            "backup": [{
                "label": "snap_false",
                "timestamp": {"start": False},
                "info": {"size": 200}
            }]
        }]
        
        # This shouldn't crash
        self.config_pg._process_snapshot_data(data_pg, "pgbackrest")

        snap = self.env["backup.snapshot"].search([("snapshot_id", "=", "snap_false")])
        self.assertEqual(len(snap), 1)
        self.assertEqual(snap.name, "snap_false")

    def test_restore_wizard_validation(self):
        snap = self.env["backup.snapshot"].create({
            "config_id": self.config_pg.id,
            "snapshot_id": "snap_test",
        })
        wizard = self.env["backup.restore.wizard"].create({
            "snapshot_id": snap.id,
            "restore_target_path": "../invalid_stanza",
        })
        with self.assertRaises(UserError):
            wizard.action_restore()
        
        wizard.restore_target_path = "valid_stanza_123"
        # Since we use safe_patch we need to patch publish_to_rabbitmq so it doesn't try to connect
        with self.safe_patch("odoo.addons.backup_management.models.restore_wizard.publish_to_rabbitmq"):
            res = wizard.action_restore()
            self.assertIsInstance(res, dict)

    def test_payload_publisher_variables(self):
        # [!] safe_patch() already calls patcher.start() and registers
        # patcher.stop() via addCleanup() -- it returns the installed
        # mock directly, not a context manager. `with self.safe_patch(...)
        # as mock_pub:` "worked" without error (MagicMock auto-supports
        # __enter__/__exit__), but silently bound mock_pub to
        # mock.__enter__.return_value -- an unrelated auto-generated
        # child mock, not the actual mock installed on the module -- so
        # this never actually asserted against the real call.
        mock_pub = self.safe_patch(
            "odoo.addons.backup_management.models.backup_config.publish_to_rabbitmq"
        )
        self.config1.with_env(self.env).action_trigger_backup()
        self.env.cr.postcommit.run()

        mock_pub.assert_called_once()
        payload = json.loads(mock_pub.call_args[0][1])
        self.assertIn("storage_type", payload)
        self.assertIn("bucket_name", payload)
        self.assertIn("endpoint_url", payload)
        self.assertIn("access_key", payload)
        self.assertIn("secret_key", payload)
        self.assertIn("kopia_password", payload)
        self.assertIn("exclude_patterns", payload)

    def test_kopia_restore_destination_ignores_free_text_target_path(self):
        # Bug-hunt fix (2026-09-13): action_restore's kopia branch used to
        # build the real filesystem restore destination straight from
        # restore_target_path, a free-text field the ir.rule multi-tenant
        # scoping on backup.snapshot/backup.config never touches. A backup
        # admin scoped to only their own website's snapshots could type
        # another tenant's own target_path basename here, and since Kopia's
        # `restore` writes INTO the given directory, this could clobber
        # another tenant's real backup repository. The fix derives the
        # destination from backup.job's own database-assigned id instead
        # (immune to injection/traversal by construction), leaving
        # restore_target_path's own field/validation in place but unused
        # for kopia. This asserts the *actual* cmd_args sent to the worker
        # use the job-id-derived path, not the attacker-controlled one --
        # not just that some restore succeeded.
        snap = self.env["backup.snapshot"].create({
            "config_id": self.config1.id,
            "snapshot_id": "snap_restore_dest",
        })
        wizard = self.env["backup.restore.wizard"].create({
            "snapshot_id": snap.id,
            # Deliberately shaped like another tenant's own target_path
            # basename -- not a shell-injection payload (that's covered by
            # test_restore_wizard_validation/test_restore_wizard_security),
            # a plausible *other config's* real destination.
            "restore_target_path": "/var/lib/odoo/backups/test_kopia1",
        })
        mock_pub = self.safe_patch(
            "odoo.addons.backup_management.models.restore_wizard.publish_to_rabbitmq"
        )
        res = wizard.action_restore()
        self.env.cr.postcommit.run()

        mock_pub.assert_called_once()
        payload = json.loads(mock_pub.call_args[0][1])
        job = self.env["backup.job"].browse(res.get("res_id"))
        expected_dest = f"/var/lib/odoo/backups/restore_{job.id}"
        self.assertEqual(
            payload["cmd_args"],
            ["kopia", "restore", "snap_restore_dest", expected_dest],
        )
        self.assertNotIn(wizard.restore_target_path, payload["cmd_args"])
        # The real destination must also be discoverable from the job
        # itself, since the operator's own restore_target_path input no
        # longer says where the data actually went.
        self.assertIn(expected_dest, job.output_log)

    def test_restore_payload_carries_the_s3_b2_storage_routing_fields(self):
        # Bug-hunt fix (2026-09-27, tier-1 pass): producer/consumer payload-schema
        # drift. daemon/main.py's kopia branch is keyed on cmd[0] == "kopia" AND
        # config.get("storage_type") in ("s3", "b2") -- that is what sets
        # KOPIA_CONFIG_PATH to this backup.config's OWN per-config repository.config
        # and calls _ensure_kopia_s3_repository() before running the command. It
        # covers restore_cmd jobs by design, but action_restore's payload never
        # carried storage_type/bucket_name/endpoint_url/access_key/secret_key at
        # all, so storage_type came back absent, defaulted to "local", and every
        # restore of an off-site S3/B2 backup ran against kopia's single GLOBAL
        # default config instead of the bucket the snapshot actually lives in.
        # Asserts on the real payload dict sent to RabbitMQ, so it fails if any of
        # the five fields goes missing again.
        s3_config = self.env["backup.config"].create({
            "name": "Config S3 Restore",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_kopia_s3",
            "storage_type": "s3",
            "bucket_name": "hams-offsite-bucket",
            "endpoint_url": "https://s3.us-west-002.backblazeb2.com",
            "access_key": "AKIAEXAMPLEONLY",
        })
        snap = self.env["backup.snapshot"].create({
            "config_id": s3_config.id,
            "snapshot_id": "snap_s3_restore",
        })
        wizard = self.env["backup.restore.wizard"].create({
            "snapshot_id": snap.id,
            "restore_target_path": "/var/lib/odoo/backups/test_kopia_s3",
        })
        mock_pub = self.safe_patch(
            "odoo.addons.backup_management.models.restore_wizard.publish_to_rabbitmq"
        )
        wizard.action_restore()
        self.env.cr.postcommit.run()

        mock_pub.assert_called_once()
        payload = json.loads(mock_pub.call_args[0][1])
        self.assertEqual(payload["storage_type"], "s3")
        self.assertEqual(payload["bucket_name"], "hams-offsite-bucket")
        self.assertEqual(
            payload["endpoint_url"], "https://s3.us-west-002.backblazeb2.com"
        )
        self.assertEqual(payload["access_key"], "AKIAEXAMPLEONLY")
        # Present as a key even when unset, so the daemon's own
        # config.get("secret_key") branch is reached rather than silently absent.
        self.assertIn("secret_key", payload)

