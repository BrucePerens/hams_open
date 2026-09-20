# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0-or-later License.
import pika

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


class _Channel:
    """Stub AMQP channel: records sends, or raises the given error."""

    def __init__(self, exc=None):
        self.exc = exc
        self.sent = []

    def basic_publish(self, **kwargs):
        if self.exc:
            raise self.exc
        self.sent.append(kwargs)


@tagged("post_install", "-at_install")
class TestBackupPublishFailure(RealTransactionCase):
    """pool.publish() reports success before the real send, so a dead broker
    used to leave the backup.job "pending: Queued in RabbitMQ..." forever.
    publish_to_rabbitmq() now passes on_result and a failed send fails the job.
    A stub channel stands in for the broker (no live RabbitMQ needed); the
    pool's real postcommit machinery, the real commit and the real job write
    (through its own cursor) are all exercised."""

    def setUp(self):
        super().setUp()
        self.env.user.group_ids |= self.env.ref("backup_management.group_backup_admin")
        self.config = self.env["backup.config"].create({
            "name": f"Publish Failure Config {self.id()}",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_publish_failure",
            "storage_type": "local",
        })
        self.pool = self.env["hams_rabbitmq.pool"]

    def _trigger(self):
        self.config.action_trigger_backup()
        job = self.env["backup.job"].search(
            [("config_id", "=", self.config.id)], limit=1, order="id desc"
        )
        self.assertTrue(job)
        self.env.cr.commit()  # runs the postcommit hooks, incl. the real send
        # The send's on_result wrote through its own cursor; end our own
        # snapshot so the re-read below sees that committed write.
        self.env.cr.rollback()
        job.invalidate_recordset()
        return job

    # Tests [@ANCHOR: backup_management:COMM_job_dispatch_failed]
    def test_01_no_channel_marks_the_job_failed(self):
        self.safe_patch_object(type(self.pool), "_get_channel", return_value=None)
        job = self._trigger()
        self.assertEqual(job.state, "failed")
        self.assertIn("NOT run", job.output_log)

    def test_02_broker_rejection_marks_the_job_failed(self):
        chan = _Channel(exc=pika.exceptions.AMQPError("broker down"))
        self.safe_patch_object(type(self.pool), "_get_channel", return_value=chan)
        job = self._trigger()
        self.assertEqual(job.state, "failed")

    def test_03_successful_send_leaves_the_job_pending(self):
        chan = _Channel()
        self.safe_patch_object(type(self.pool), "_get_channel", return_value=chan)
        job = self._trigger()
        self.assertEqual(len(chan.sent), 1)
        self.assertEqual(job.state, "pending")

    def test_04_a_job_already_taken_by_a_worker_is_not_overwritten(self):
        job = self.env["backup.job"].create({
            "config_id": self.config.id, "job_type": "kopia", "state": "processing",
        })
        job._mark_dispatch_failed()
        self.assertEqual(job.state, "processing")
