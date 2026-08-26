# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0-or-later License.
import json
import time

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestBackupRabbitMQIntegration(RealTransactionCase):
    """
    action_trigger_backup() defers publish_to_rabbitmq() to a cr.postcommit
    hook (ADR-0071's Asynchronous Bastion Pattern). The existing coverage in
    test_batch_2.py mocks publish_to_rabbitmq() itself and only proves the
    right Python function was called with the right args -- it never proves
    a message actually reaches RabbitMQ, and a normal (rolled-back)
    TransactionCase can't prove that either, since the postcommit hook only
    fires on a genuine commit. This uses RealTransactionCase (per MASTER_12
    Section 9's Anti-Mocking mandate) to consume the real message back off
    the real "backup_tasks" queue that publish_to_rabbitmq() actually
    publishes to.
    """

    def setUp(self):
        super().setUp()
        self.env.user.group_ids |= self.env.ref(
            "backup_management.group_backup_admin"
        )
        self.config = self.env["backup.config"].create({
            "name": f"RMQ Integration Config {self.id()}",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_rmq_integration",
            "storage_type": "local",
        })

    def test_trigger_backup_delivers_a_real_message(self):
        pool = self.env["hams_rabbitmq.pool"]
        channel = pool._get_channel()
        channel.queue_declare(queue="backup_tasks", durable=True)
        channel.queue_purge("backup_tasks")

        self.config.action_trigger_backup()
        job = self.env["backup.job"].search(
            [("config_id", "=", self.config.id)], limit=1, order="id desc"
        )
        self.assertTrue(job, "action_trigger_backup() must create a backup.job.")
        self.env.cr.commit()

        received = None
        deadline = time.time() + 15.0
        while time.time() < deadline:
            method, _props, body = channel.basic_get("backup_tasks", auto_ack=True)
            if method:
                payload = json.loads(body)
                if payload.get("job_id") == job.id:
                    received = payload
                    break
            else:
                time.sleep(0.25)  # audit-ignore-sleep

        self.assertIsNotNone(
            received,
            "action_trigger_backup() must actually deliver a message to the "
            "real backup_tasks RabbitMQ queue once the transaction commits, "
            "not just call publish_to_rabbitmq() in-process.",
        )
        self.assertEqual(received["config_id"], self.config.id)
        self.assertEqual(received["config_engine"], "kopia")
        self.assertIn("storage_type", received)
        self.assertIn("bucket_name", received)
        self.assertIn("endpoint_url", received)
        self.assertIn("access_key", received)
        self.assertIn("secret_key", received)
        self.assertIn("kopia_password", received)
        self.assertIn("exclude_patterns", received)
