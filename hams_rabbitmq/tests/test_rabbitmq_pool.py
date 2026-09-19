# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later
import json
import time
import uuid

import pika

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestRabbitMQPool(RealTransactionCase):
    """
    hams_rabbitmq.pool had zero test coverage before this. It's also a
    direct consumer of zero_sudo.security.utils._get_system_param() for
    rabbitmq.user/pass/port/vhost -- exactly the class of key that was
    silently returning the caller's default instead of the real configured
    value (see ham_base/models/ir_config_parameter.py and
    zero_sudo/models/security_utils.py). These tests exercise the real
    connection pool against the real local RabbitMQ instance rather than
    mocking pika, so a regression in that config-resolution chain would
    show up here as a real connection/publish failure, not a passing mock.
    """

    def test_01_get_channel_connects_with_real_config(self):
        # [@ANCHOR: COMM_test_01_get_channel_connects_with_real_config]

        # Tests [@ANCHOR: rabbitmq_get_channel]
        pool = self.env["hams_rabbitmq.pool"]
        channel = pool._get_channel()
        self.assertIsNotNone(
            channel,
            "_get_channel() must return a real channel using the "
            "rabbitmq.* parameters resolved via _get_system_param().",
        )
        self.assertTrue(channel.is_open, "the returned channel must be open.")

    def test_02_publish_delivers_a_real_message_after_commit(self):
        # [@ANCHOR: COMM_test_02_publish_delivers_a_real_message_after_commit]
        """
        publish() defers the actual send to a cr.postcommit hook, which
        only fires on a genuine commit -- RealTransactionCase is required
        here, not a normal (rolled-back) TransactionCase, or this hook
        would silently never run and the test would prove nothing.
        """
        # Tests [@ANCHOR: rabbitmq_publish]
        pool = self.env["hams_rabbitmq.pool"]
        queue_name = f"hams_rabbitmq_test_{uuid.uuid4().hex[:12]}"

        setup_channel = pool._get_channel()
        setup_channel.queue_declare(queue=queue_name, durable=False, auto_delete=True)
        setup_channel.queue_purge(queue_name)

        payload = {"marker": queue_name, "value": 42}
        pool.publish("", queue_name, payload)
        self.env.cr.commit()

        received = None
        deadline = time.time() + 15.0
        while time.time() < deadline:
            method, _props, body = setup_channel.basic_get(queue_name, auto_ack=True)
            if method:
                received = json.loads(body)
                break
            time.sleep(0.25)  # audit-ignore-sleep

        setup_channel.queue_delete(queue_name)

        self.assertIsNotNone(
            received,
            "publish() must actually deliver the message to RabbitMQ once "
            "the transaction commits.",
        )
        self.assertEqual(received, payload)

    def test_03_publish_serializes_dict_bodies_to_json(self):
        # [@ANCHOR: COMM_test_03_publish_serializes_dict_bodies_to_json]
        """
        publish() special-cases dict bodies (json.dumps), but a raw string
        body must pass through unchanged -- verify both, since a silent
        double-encode or a missed encode would corrupt every consumer's
        parsing without necessarily crashing publish() itself.
        """
        # Tests [@ANCHOR: rabbitmq_publish]
        pool = self.env["hams_rabbitmq.pool"]
        queue_name = f"hams_rabbitmq_test_{uuid.uuid4().hex[:12]}"

        setup_channel = pool._get_channel()
        setup_channel.queue_declare(queue=queue_name, durable=False, auto_delete=True)

        pool.publish("", queue_name, "plain-string-body")
        self.env.cr.commit()

        received = None
        deadline = time.time() + 15.0
        while time.time() < deadline:
            method, _props, body = setup_channel.basic_get(queue_name, auto_ack=True)
            if method:
                received = body.decode("utf-8")
                break
            time.sleep(0.25)  # audit-ignore-sleep

        setup_channel.queue_delete(queue_name)

        self.assertEqual(received, "plain-string-body")

    def test_04_get_channel_recreates_a_closed_channel_on_an_open_connection(self):
        # [@ANCHOR: COMM_test_04_get_channel_recreates_a_closed_channel_on_an_open_connection]
        """
        _get_channel()'s elif branch (connection open, channel closed)
        was never exercised -- only the initial "no connection yet" path
        was. This is a real self-healing case: RabbitMQ (or a proxy)
        closing an individual channel while the underlying connection
        stays up is a normal, documented AMQP event (e.g. a channel-level
        protocol error), and the pool's singleton pattern means a stale
        closed channel would otherwise wedge every future publish() until
        the whole process restarted.
        """
        # Tests [@ANCHOR: rabbitmq_get_channel]
        pool = self.env["hams_rabbitmq.pool"]
        first_channel = pool._get_channel()
        self.assertTrue(first_channel.is_open)

        first_channel.close()
        self.assertTrue(first_channel.is_closed)

        second_channel = pool._get_channel()
        self.assertIsNotNone(second_channel)
        self.assertTrue(second_channel.is_open, "a fresh channel must be created on the still-open connection")
        self.assertIsNot(
            second_channel, first_channel,
            "must not hand back the same closed channel object",
        )

    def test_05_on_result_reports_real_success_and_failure_after_commit(self):
        # Regression (0fd43f88): publish() always returned True before the
        # deferred send ran, so no caller could ever see a real failure.
        # Uses a stub channel so it needs no live broker.
        # Tests [@ANCHOR: rabbitmq_publish]
        pool = self.env["hams_rabbitmq.pool"]

        class _Channel:
            def __init__(self, exc=None):
                self.exc = exc
                self.sent = []

            def basic_publish(self, **kwargs):
                if self.exc:
                    raise self.exc
                self.sent.append(kwargs)

        good = _Channel()
        get_channel = self.safe_patch_object(type(pool), "_get_channel", return_value=good)
        results = []
        self.assertTrue(pool.publish("", "q", "ok", on_result=results.append))
        self.assertEqual(results, [], "on_result must not fire before the commit")
        self.env.cr.commit()
        self.assertEqual(results, [True])
        self.assertEqual(len(good.sent), 1)

        # Broker rejects the publish: on_result(False), publish() itself still True.
        get_channel.return_value = _Channel(exc=pika.exceptions.AMQPError("broker down"))
        failures = []
        self.assertTrue(pool.publish("", "q", "lost", on_result=failures.append))
        self.env.cr.commit()
        self.assertEqual(failures, [False])

        # No channel at all (broker unreachable): on_result(False).
        get_channel.return_value = None
        no_channel = []
        self.assertTrue(pool.publish("", "q", "lost", on_result=no_channel.append))
        self.env.cr.commit()
        self.assertEqual(no_channel, [False])

    def test_06_a_failing_on_result_callback_does_not_break_the_commit(self):
        # Tests [@ANCHOR: rabbitmq_publish]
        pool = self.env["hams_rabbitmq.pool"]

        def boom(_success):
            raise RuntimeError("callback bug")

        self.safe_patch_object(type(pool), "_get_channel", return_value=None)
        pool.publish("", "hams_rabbitmq_unused", "x", on_result=boom)
        self.env.cr.commit()  # must not raise
