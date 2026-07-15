# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
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
        with self.safe_patch("odoo.addons.backup_management.models.backup_config.publish_to_rabbitmq") as mock_pub:
            self.config1.action_trigger_backup()
            for func in list(self.env.cr.postcommit):
                func()
            self.env.cr.postcommit.clear()
            
            mock_pub.assert_called_once()
            payload = json.loads(mock_pub.call_args[0][1])
            self.assertIn("storage_type", payload)
            self.assertIn("bucket_name", payload)
            self.assertIn("endpoint_url", payload)
            self.assertIn("access_key", payload)
            self.assertIn("secret_key", payload)
            self.assertIn("kopia_password", payload)
            self.assertIn("exclude_patterns", payload)

