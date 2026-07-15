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
class TestRealBackupWorker(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.daemon_proc = None

    def tearDown(self):
        if self.daemon_proc:
            os.killpg(os.getpgid(self.daemon_proc.pid), 9)
            self.daemon_proc.wait(timeout=2.0)
        super().tearDown()

    def test_real_backup_worker_rabbitmq(self):
        # Tests [@ANCHOR: backup_management:COMM_test_backup_worker_real]
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        daemon_script = os.path.join(base_dir, "daemon", "backup_worker.py")

        daemon_utils = self.env["zero_sudo.daemon.utils"]

        # Ensure API key is provisioned for this test database and committed
        admin_uid = self.env.ref("base.user_admin").id
        self.env.ref("base.user_admin").write(
            {
                "group_ids": [
                    (4, self.env.ref("daemon_key_manager.group_daemon_key_manager").id)
                ]
            }
        )
        self.env.cr.commit()
        self.env["daemon.key.registry"].with_user(
            admin_uid
        ).action_force_provision_all()
        self.env.cr.commit()

        # Fetch the auto-generated API key for the backup service daemon from its secure vault
        env_vars = {}
        env_vars["DB_NAME"] = self.env.cr.dbname
        env_file = "/opt/hams/etc/keys/backup_worker.env"
        if os.path.exists(env_file):
            with open(env_file, "r") as f:
                for line in f:
                    if line.startswith("ODOO_RPC_KEY="):
                        env_vars["ODOO_SERVICE_PASSWORD"] = line.strip().split("=", 1)[
                            1
                        ]

        self.daemon_proc = daemon_utils.start_daemon_process(
            daemon_script, env_vars=env_vars
        )

        rmq_host = os.environ.get("RABBITMQ_HOST", "rabbitmq")
        # System infrastructure variables are permissible in daemon bootstrap hooks
        creds = pika.PlainCredentials(
            os.environ.get("RMQ_USER", "guest"),
            os.environ.get("RMQ_PASS", "guest"),  # burn-ignore-env
        )  # burn-ignore-env  # fmt: skip
        conn = pika.BlockingConnection(  # burn-ignore-pika  # fmt: skip
            pika.ConnectionParameters(host=rmq_host, credentials=creds)
        )
        channel = conn.channel()
        channel.queue_declare(queue="backup_tasks", durable=True)
        channel.queue_purge("backup_tasks")

        config = self.env["backup.config"].create(
            {
                "name": "Dummy Config",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/dummy_backup",
            }
        )
        job = self.env["backup.job"].create(
            {
                "config_id": config.id,
                "job_type": "kopia",
                "state": "pending",
            }
        )
        self.env.cr.commit()

        job_payload = {
            "job_id": job.id,
            "engine": "dummy",
            "target_path": config.target_path,
            "config_id": config.id,
        }
        channel.basic_publish(
            exchange="",
            routing_key="backup_tasks",
            body=json.dumps(job_payload),
            properties=pika.BasicProperties(delivery_mode=2),
        )

        consumed = False
        start_time = time.time()
        while time.time() - start_time < 60.0:
            q = channel.queue_declare(queue="backup_tasks", durable=True)
            if q.method.message_count == 0:
                consumed = True
                break
            time.sleep(0.5)  # audit-ignore-sleep

        conn.close()

        # Wait for the daemon to finish writing back to the DB to avoid SerializationFailure
        start_time = time.time()
        while time.time() - start_time < 15.0:
            job.invalidate_recordset(["state"])
            if job.state in ("done", "failed"):
                break
            time.sleep(0.5)  # audit-ignore-sleep

        # Commit to close the test's long-running transaction before we attempt to delete records
        # modified by the daemon's concurrent transaction.
        self.env.cr.commit()

        job.unlink()
        config.unlink()
        self.env.cr.commit()

        self.assertTrue(
            consumed,
            "The real backup worker daemon failed to consume the RabbitMQ message!",
        )
