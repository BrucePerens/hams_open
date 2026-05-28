# -*- coding: utf-8 -*-
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
            try:
                self.daemon_proc.terminate()
                self.daemon_proc.wait(timeout=2.0)
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Daemon termination ignored: %s", repr(e))
        super().tearDown()

    def test_real_backup_worker_rabbitmq(self):
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        daemon_script = os.path.join(base_dir, "daemon", "backup_worker.py")

        daemon_utils = self.env["zero_sudo.daemon.utils"]
        self.daemon_proc = daemon_utils.start_daemon_process(daemon_script)

        rmq_host = os.environ.get("RABBITMQ_HOST", "rabbitmq")
        creds = pika.PlainCredentials(os.environ.get("RMQ_USER", "guest"), os.environ.get("RMQ_PASS", "guest"))
        conn = pika.BlockingConnection(pika.ConnectionParameters(host=rmq_host, credentials=creds))
        channel = conn.channel()
        channel.queue_declare(queue="backup_jobs", durable=True)
        channel.queue_purge("backup_jobs")

        job_payload = {
            "job_id": 999,
            "engine": "dummy",
            "target_path": "/var/lib/odoo/backups/dummy_backup",
            "config_id": 1
        }
        channel.basic_publish(
            exchange="",
            routing_key="backup_jobs",
            body=json.dumps(job_payload),
            properties=pika.BasicProperties(delivery_mode=2)
        )

        consumed = False
        start_time = time.time()
        while time.time() - start_time < 60.0:
            q = channel.queue_declare(queue="backup_jobs", durable=True)
            if q.method.message_count == 0:
                consumed = True
                break
            time.sleep(0.5)  # audit-ignore-sleep

        conn.close()
        self.assertTrue(consumed, "The real backup worker daemon failed to consume the RabbitMQ message!")
