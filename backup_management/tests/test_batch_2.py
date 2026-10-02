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
        # Tests [@ANCHOR: backup_management:restore_wizard_refuses_pgbackrest]
        # Updated 2026-10-01: a pgbackrest restore through this wizard now ALWAYS raises
        # UserError, regardless of how well-formed restore_target_path is -- see
        # action_restore()'s own matching comment for why (the restore path is never actually
        # routed through the privileged sidecar that can write PostgreSQL's real data
        # directory). This test used to prove an invalid stanza name was refused but a valid
        # one succeeded; now it proves EVERY pgbackrest restore attempt is refused, which is
        # the whole point of this fix -- a well-formed stanza name is no longer enough to reach
        # the (now-unreachable) cmd_args construction this file's own `elif` branch still
        # documents as a blueprint for the real, future sidecar-routed fix.
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
        with self.assertRaises(UserError):
            wizard.action_restore()

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

    def test_region_field_reaches_the_published_payload(self):
        # night_shift_todo/low/backup-config-no-region-field-6e8c1a4f.md:
        # backup.config used to have no `region` field at all, so the
        # pgbackrest daemon's own real `config.get('region') or
        # 'us-east-1'` override (daemon/main.py's
        # _pgbackrest_s3_repo_args(), tested there by
        # test_03_region_override_honored) was dead capability -- there was
        # no way for an admin to actually populate it. This proves the
        # model-side half of the fix: a non-default region set on the
        # config is carried through to the exact payload key
        # (config["region"]) the daemon's override reads.
        config = self.env["backup.config"].create({
            "name": f"Region Config {self.id()}",
            "engine": "pgbackrest",
            "target_path": "region_test_stanza",
            "storage_type": "s3",
            "region": "eu-central-1",
        })
        mock_pub = self.safe_patch(
            "odoo.addons.backup_management.models.backup_config.publish_to_rabbitmq"
        )
        config.with_env(self.env).action_trigger_backup()
        self.env.cr.postcommit.run()

        mock_pub.assert_called_once()
        payload = json.loads(mock_pub.call_args[0][1])
        self.assertEqual(payload.get("region"), "eu-central-1")

    def test_region_field_defaults_to_falsy_when_unset(self):
        # A config with no region set must still publish a "region" key
        # (so the daemon's `config.get('region') or 'us-east-1'` override
        # sees an explicit falsy value and falls back to the documented
        # default) rather than omitting the key entirely.
        mock_pub = self.safe_patch(
            "odoo.addons.backup_management.models.backup_config.publish_to_rabbitmq"
        )
        self.config1.with_env(self.env).action_trigger_backup()
        self.env.cr.postcommit.run()

        mock_pub.assert_called_once()
        payload = json.loads(mock_pub.call_args[0][1])
        self.assertIn("region", payload)
        self.assertFalse(payload.get("region"))

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

