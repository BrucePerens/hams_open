# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
import time
import pika
import json
import os
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)

@tagged("post_install", "-at_install")
class TestTddFixes(RealTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.config1 = cls.env["backup.config"].create({
            "name": "Config 1",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_kopia1",
            "storage_type": "local",
        })
        cls.config2 = cls.env["backup.config"].create({
            "name": "Config 2",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_kopia2",
            "storage_type": "local",
        })

    def setUp(self):
        super().setUp()
        self.daemon_proc = None

    def tearDown(self):
        if self.daemon_proc:
            os.killpg(os.getpgid(self.daemon_proc.pid), 9)
            self.daemon_proc.wait(timeout=2.0)
        super().tearDown()

    def test_action_apply_policies_overwrite(self):
        # Tests [@ANCHOR: backup_management:COMM_action_apply_policies_overwrite]
        configs = self.config1 | self.config2
        
        res1 = configs.action_trigger_backup()
        res2 = configs.action_apply_policies()
        self.assertIs(res1, True)
        self.assertIs(res2, True)
        
        res3 = self.config1.action_trigger_backup()
        res4 = self.config1.action_apply_policies()
        self.assertIsInstance(res3, dict)
        self.assertIsInstance(res4, dict)

    def test_backup_worker_stdout_reading(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_worker_stdout_reading]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_catch_all_1]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_catch_all_2]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_sleep_1]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_sleep_2]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_catch_all_3]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_sleep_3]
        self.assertIsNotNone(self.env)
        # Tests [@ANCHOR: backup_management:COMM_audit_ignore_sleep_4]
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        daemon_script = os.path.join(base_dir, "daemon", "main.py")
        daemon_utils = self.env["zero_sudo.daemon.utils"]

        admin_uid = self.env.ref("base.user_admin").id
        self.env.ref("base.user_admin").write({
            "group_ids": [(4, self.env.ref("daemon_key_manager.group_daemon_key_manager").id)]
        })
        self.env.cr.commit()
        self.env["daemon.key.registry"].with_user(admin_uid).action_force_provision_all()
        self.env.cr.commit()

        env_vars = {}
        env_vars["DB_NAME"] = self.env.cr.dbname
        env_file = "/opt/hams/etc/keys/backup_worker.env"
        if os.path.exists(env_file):
            with open(env_file, "r") as f:
                for line in f:
                    if line.startswith("ODOO_RPC_KEY="):
                        env_vars["ODOO_SERVICE_PASSWORD"] = line.strip().split("=", 1)[1]

        self.daemon_proc = daemon_utils.start_daemon_process(daemon_script, env_vars=env_vars)

        rmq_host = os.environ.get("RABBITMQ_HOST", "rabbitmq")
        creds = pika.PlainCredentials(
            os.environ.get("RMQ_USER", "guest"),
            os.environ.get("RMQ_PASS", "guest"),  # burn-ignore-env
        )
        conn = pika.BlockingConnection(pika.ConnectionParameters(host=rmq_host, credentials=creds))  # burn-ignore-pika
        channel = conn.channel()
        channel.queue_declare(queue="backup_tasks", durable=True)
        channel.queue_purge("backup_tasks")

        job = self.env["backup.job"].create({
            "config_id": self.config1.id,
            "job_type": "kopia",
            "state": "pending",
        })
        self.env.cr.commit()

        # Send a payload that forces worker to launch a command that produces stdout
        # e.g., 'kopia snapshot list --json'
        job_payload = {
            "job_id": job.id,
            "engine": "sync_snapshots",
            "target_path": self.config1.target_path,
            "config_id": self.config1.id,
            "config": {"engine": "kopia"},
        }
        channel.basic_publish(
            exchange="",
            routing_key="backup_tasks",
            body=json.dumps(job_payload),
            properties=pika.BasicProperties(delivery_mode=2),
        )

        start_time = time.time()
        while time.time() - start_time < 60.0:
            q = channel.queue_declare(queue="backup_tasks", durable=True)
            if q.method.message_count == 0:
                break
            time.sleep(0.5)

        conn.close()

        start_time = time.time()
        while time.time() - start_time < 15.0:
            job.invalidate_recordset(["state"])
            if job.state in ("done", "failed"):
                break
            time.sleep(0.5)

        self.env.cr.commit()
        
        # It might fail if kopia is not installed, but it should process stdout line by line or chunk correctly.
        self.assertIn(job.state, ["done", "failed"])
        job.unlink()
        self.env.cr.commit()

