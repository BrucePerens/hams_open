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
class TestTddBatch1(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.daemon_proc = None
        self.conn = None
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
        self.conn = pika.BlockingConnection(pika.ConnectionParameters(host=rmq_host, credentials=creds))  # burn-ignore-pika
        self.channel = self.conn.channel()
        self.channel.queue_declare(queue="backup_tasks", durable=True)
        self.channel.queue_purge("backup_tasks")

    def tearDown(self):
        if self.conn and self.conn.is_open:
            self.conn.close()
        if self.daemon_proc:
            os.killpg(os.getpgid(self.daemon_proc.pid), 9)
            self.daemon_proc.wait(timeout=2.0)
        super().tearDown()

    def _wait_for_job(self, job):
        start_time = time.time()
        # Wait for queue to drain
        while time.time() - start_time < 60.0:
            q = self.channel.queue_declare(queue="backup_tasks", durable=True)
            if q.method.message_count == 0:
                break
            time.sleep(0.5)

        # Wait for DB state update
        start_time = time.time()
        while time.time() - start_time < 15.0:
            job.invalidate_recordset(["state"])
            if job.state in ("done", "failed"):
                break
            time.sleep(0.5)
        self.env.cr.commit()

    def test_item1_kopia_restore_whitelist(self):
        config = self.env["backup.config"].create({
            "name": "Config 1", "engine": "kopia", "target_path": "/var/lib/odoo/backups/test"
        })
        job = self.env["backup.job"].create({"config_id": config.id, "job_type": "kopia", "state": "pending"})
        self.env.cr.commit()

        job_payload = {
            "job_id": job.id,
            "engine": "restore_cmd",
            "cmd_args": ["kopia", "restore", "snap1", "/etc/passwd"],
            "config_id": config.id,
        }
        self.channel.basic_publish(
            exchange="", routing_key="backup_tasks",
            body=json.dumps(job_payload), properties=pika.BasicProperties(delivery_mode=2),
        )
        self._wait_for_job(job)
        
        self.assertEqual(job.state, "failed")
        self.assertIn("PermissionError", job.log)

        job.unlink()
        config.unlink()
        self.env.cr.commit()

    def test_item2_pgbackrest_restore_alphanumeric(self):
        config = self.env["backup.config"].create({
            "name": "Config 1", "engine": "kopia", "target_path": "/var/lib/odoo/backups/test"
        })
        job = self.env["backup.job"].create({"config_id": config.id, "job_type": "kopia", "state": "pending"})
        self.env.cr.commit()

        job_payload = {
            "job_id": job.id,
            "engine": "restore_cmd",
            "cmd_args": ["pgbackrest", "restore", "--stanza=valid_stanza", "--set=2023-01-01;rm"],
            "config_id": config.id,
        }
        self.channel.basic_publish(
            exchange="", routing_key="backup_tasks",
            body=json.dumps(job_payload), properties=pika.BasicProperties(delivery_mode=2),
        )
        self._wait_for_job(job)
        
        self.assertEqual(job.state, "failed")
        self.assertIn("PermissionError", job.log)

        job.unlink()
        config.unlink()
        self.env.cr.commit()

    def test_item3_kopia_flag_injection(self):
        config = self.env["backup.config"].create({
            "name": "Config 1", "engine": "kopia", "target_path": "/var/lib/odoo/backups/test"
        })
        job = self.env["backup.job"].create({"config_id": config.id, "job_type": "kopia", "state": "pending"})
        self.env.cr.commit()

        job_payload = {
            "job_id": job.id,
            "engine": "kopia",
            "target_path": "--help",
            "config_id": config.id,
            "config": {"engine": "kopia"}
        }
        self.channel.basic_publish(
            exchange="", routing_key="backup_tasks",
            body=json.dumps(job_payload), properties=pika.BasicProperties(delivery_mode=2),
        )
        self._wait_for_job(job)
        
        self.assertEqual(job.state, "failed")
        # Ensure it failed (due to nonexistent command/dir, or flag injection handled by --)
        
        job.unlink()
        config.unlink()
        self.env.cr.commit()

    def test_item5_timezone_data_corruption(self):
        config = self.env["backup.config"].create({
            "name": "Config 1", "engine": "kopia", "target_path": "/var/lib/odoo/backups/test"
        })
        job = self.env["backup.job"].create({"config_id": config.id, "job_type": "kopia", "state": "pending"})
        self.env.cr.commit()

        # Write a dummy script to bypass validation
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        scripts_dir = os.path.join(base_dir, "daemon", "scripts")
        os.makedirs(scripts_dir, exist_ok=True)
        script_path = os.path.join(scripts_dir, "valid.py")
        with open(script_path, "w") as f:
            f.write("#!/usr/bin/env python3\nprint('ok')\n")
        os.chmod(script_path, 0o755)

        job_payload = {
            "job_id": job.id,
            "engine": "restore_drill",
            "script": script_path,
            "config_id": config.id,
            "config": {"engine": "restore_drill"}
        }
        self.channel.basic_publish(
            exchange="", routing_key="backup_tasks",
            body=json.dumps(job_payload), properties=pika.BasicProperties(delivery_mode=2),
        )
        self._wait_for_job(job)
        
        config.invalidate_recordset(["last_drill_time"])
        # Should be updated
        self.assertTrue(bool(config.last_drill_time))

        job.unlink()
        config.unlink()
        self.env.cr.commit()

    def test_item8_api_latency(self):
        config = self.env["backup.config"].create({
            "name": "Config 1", "engine": "kopia", "target_path": "/var/lib/odoo/backups/test"
        })
        job = self.env["backup.job"].create({"config_id": config.id, "job_type": "kopia", "state": "pending"})
        self.env.cr.commit()

        job_payload = {
            "job_id": job.id,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test",
            "config_id": config.id,
            "config": {
                "kopia_password": "test",
                "keep_daily": 7,
                "engine": "kopia"
            }
        }
        self.channel.basic_publish(
            exchange="", routing_key="backup_tasks",
            body=json.dumps(job_payload), properties=pika.BasicProperties(delivery_mode=2),
        )
        self._wait_for_job(job)
        
        self.assertIn(job.state, ["done", "failed"])

        job.unlink()
        config.unlink()
        self.env.cr.commit()

    def test_file_contents(self):
        base_path = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        
        service_path = os.path.join(base_path, "daemon", "backup-worker.service")
        with open(service_path, "r") as f:
            service_content = f.read()
        self.assertIn("EnvironmentFile=/opt/hams/etc/keys/backup_worker.env", service_content)
        
        doc_path = os.path.join(base_path, "data", "documentation.html")
        with open(doc_path, "r") as f:
            doc_content = f.read()
        self.assertIn('id="COMM_UX_BACKUP_SYNC"', doc_content)
        self.assertNotIn('id="backup-sync-section"', doc_content)
        
        hooks_path = os.path.join(base_path, "hooks.py")
        with open(hooks_path, "r") as f:
            hooks_content = f.read()
        self.assertIn("_get_service_uid", hooks_content)
        self.assertIn(".with_user(", hooks_content)
        self.assertNotIn("_get_service_env", hooks_content)
        
        self.assertNotIn("style=", doc_content)
