# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.hams_rabbitmq.models import rabbitmq_pool


@tagged("post_install", "-at_install")
class TestPoolWithoutCredentials(HamsTransactionCase):
    """With no RabbitMQ account configured the pool opens no connection (and never tries a default account)."""

    def test_no_credentials_means_no_channel_and_no_connection_attempt(self):
        # Tests [@ANCHOR: rabbitmq_get_channel]
        # Tests [@ANCHOR: rabbitmq_resolve_credentials]
        pool = self.env["hams_rabbitmq.pool"]
        self.safe_patch_object(pool.__class__, "_connection", None)
        self.safe_patch_object(pool.__class__, "_channel", None)
        self.safe_patch_object(rabbitmq_pool, "resolve_rabbitmq_credentials", return_value=None)
        connect = self.safe_patch_object(rabbitmq_pool.pika, "BlockingConnection")
        with self.assertLogs(rabbitmq_pool._logger, "ERROR") as logs:
            self.assertIsNone(pool._get_channel())
        self.assertIn("No RabbitMQ credentials", logs.output[0])
        connect.assert_not_called()

    def test_a_broker_error_gives_no_channel_instead_of_raising(self):
        # Tests [@ANCHOR: rabbitmq_get_channel]
        pool = self.env["hams_rabbitmq.pool"]
        self.safe_patch_object(pool.__class__, "_connection", None)
        self.safe_patch_object(pool.__class__, "_channel", None)
        self.safe_patch_object(rabbitmq_pool, "resolve_rabbitmq_credentials", return_value=("someone", "secret"))
        self.safe_patch_object(rabbitmq_pool.pika, "BlockingConnection", side_effect=rabbitmq_pool.pika.exceptions.AMQPConnectionError())
        with self.assertLogs(rabbitmq_pool._logger, "ERROR") as logs:
            self.assertIsNone(pool._get_channel())
        self.assertIn("Failed to connect to RabbitMQ", logs.output[0])
        self.assertNotIn("secret", "".join(logs.output))
