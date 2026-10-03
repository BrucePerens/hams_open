# SPDX-License-Identifier: AGPL-3.0-or-later
import json
import time
import uuid

import pika

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.addons.hams_rabbitmq.models.rabbitmq_pool import resolve_rabbitmq_credentials


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

    def test_07_publish_retries_once_on_a_fresh_connection_after_a_stale_one(self):
        # [@ANCHOR: COMM_test_07_publish_retries_once_on_a_fresh_connection_after_a_stale_one]
        """
        Production, 2026-10-03 06:51: the broker had dropped Odoo's pooled
        connection (its user was deleted), the pool still believed it open,
        basic_publish raised StreamLostError, and backup job 145 was marked
        failed. The next publish an hour later opened a fresh connection and
        worked. Here the pooled connection/channel are replaced by ones that
        look open but fail exactly that way; the real _get_channel() must then
        open a real, fresh connection and the message must really arrive.
        """
        # Tests [@ANCHOR: rabbitmq_publish]
        # Tests [@ANCHOR: rabbitmq_discard_stale_connection]
        pool = self.env["hams_rabbitmq.pool"]
        pool_class = type(pool)
        queue_name = f"hams_rabbitmq_test_{uuid.uuid4().hex[:12]}"

        setup_channel = pool._get_channel()
        self.assertIsNotNone(setup_channel, "this test needs the real local RabbitMQ")
        setup_channel.queue_declare(queue=queue_name, durable=False, auto_delete=True)
        setup_connection = pool_class._connection

        class _DeadConnection:
            is_closed = False  # the pool cannot tell: it only learns on use
            closed = 0

            def channel(self):
                raise pika.exceptions.StreamLostError("stale")

            def close(self):
                _DeadConnection.closed += 1
                raise pika.exceptions.StreamLostError(
                    "Stream connection lost: ConnectionResetError(104, 'Connection reset by peer')"
                )

        class _DeadChannel:
            is_closed = False
            attempts = 0

            def basic_publish(self, **kwargs):
                _DeadChannel.attempts += 1
                raise pika.exceptions.StreamLostError(
                    "Stream connection lost: ConnectionResetError(104, 'Connection reset by peer')"
                )

        dead_channel = _DeadChannel()
        self.safe_patch_object(pool_class, "_connection", _DeadConnection())
        self.safe_patch_object(pool_class, "_channel", dead_channel)

        results = []
        payload = {"marker": queue_name, "retried": True}
        self.assertTrue(pool.publish("", queue_name, payload, on_result=results.append))
        with self.assertLogs("odoo.addons.hams_rabbitmq.models.rabbitmq_pool", "WARNING") as logs:
            self.env.cr.commit()
        self.assertTrue(
            any("retrying once on a fresh connection" in line for line in logs.output),
            "the dead connection must be logged, not silently replaced",
        )

        fresh_connection = pool_class._connection
        self.assertEqual(_DeadChannel.attempts, 1, "the stale channel is tried exactly once")
        self.assertEqual(_DeadConnection.closed, 1, "the dead connection is discarded (closed)")
        self.assertIsNotNone(fresh_connection)
        self.assertNotIsInstance(fresh_connection, _DeadConnection)
        self.assertTrue(fresh_connection.is_open, "a fresh real connection replaced the dead one")
        self.assertIsNot(pool_class._channel, dead_channel)
        self.assertEqual(results, [True], "the retry on the fresh connection must succeed")

        received = None
        deadline = time.time() + 15.0
        while time.time() < deadline:
            method, _props, body = setup_channel.basic_get(queue_name, auto_ack=True)
            if method:
                received = json.loads(body)
                break
            time.sleep(0.25)  # audit-ignore-sleep
        setup_channel.queue_delete(queue_name)
        if fresh_connection is not setup_connection:
            fresh_connection.close()
        self.assertEqual(received, payload, "the retried message must really reach the broker")

    def test_08_publish_reports_failure_when_the_retry_also_fails(self):
        # [@ANCHOR: COMM_test_08_publish_reports_failure_when_the_retry_also_fails]
        """
        Stub channels, no live broker needed. A stale-connection error gets
        exactly one retry: success on the retry is success, a second failure
        is reported as failure (never swallowed, never a third attempt), and
        an ordinary broker refusal that is not a dead connection is not
        retried at all.
        """
        # Tests [@ANCHOR: rabbitmq_publish]
        # Tests [@ANCHOR: rabbitmq_discard_stale_connection]
        pool = self.env["hams_rabbitmq.pool"]

        class _Channel:
            def __init__(self, exc=None):
                self.exc = exc
                self.attempts = 0

            def basic_publish(self, **kwargs):
                self.attempts += 1
                if self.exc:
                    raise self.exc

        # Stale, then a good fresh connection: delivered on the retry.
        stale = _Channel(pika.exceptions.StreamLostError("reset"))
        good = _Channel()
        get_channel = self.safe_patch_object(type(pool), "_get_channel", side_effect=[stale, good])
        results = []
        pool.publish("", "q", "x", on_result=results.append)
        self.env.cr.commit()
        self.assertEqual(results, [True])
        self.assertEqual((stale.attempts, good.attempts, get_channel.call_count), (1, 1, 2))

        # Stale, and the fresh connection fails too: reported as a failure.
        for first_error, second_error in (
            (pika.exceptions.StreamLostError("reset"), pika.exceptions.ConnectionClosedByBroker(320, "gone")),
            (ConnectionResetError(104, "Connection reset by peer"), pika.exceptions.ChannelClosed(406, "x")),
        ):
            first, second = _Channel(first_error), _Channel(second_error)
            get_channel = self.safe_patch_object(
                type(pool), "_get_channel", side_effect=[first, second, AssertionError("no third try")]
            )
            results = []
            pool.publish("", "q", "x", on_result=results.append)
            self.env.cr.commit()
            self.assertEqual(results, [False], f"{first_error!r} then {second_error!r}")
            self.assertEqual((first.attempts, second.attempts, get_channel.call_count), (1, 1, 2))

        # Stale, and no fresh connection can be opened: a failure.
        stale = _Channel(pika.exceptions.ConnectionClosed(320, "gone"))
        self.safe_patch_object(type(pool), "_get_channel", side_effect=[stale, None])
        results = []
        pool.publish("", "q", "x", on_result=results.append)
        self.env.cr.commit()
        self.assertEqual(results, [False])

        # Not a dead connection: no retry.
        refused = _Channel(pika.exceptions.UnroutableError([]))
        # (UnroutableError is an AMQPChannelError, yet the channel is alive.)
        get_channel = self.safe_patch_object(type(pool), "_get_channel", side_effect=[refused, _Channel()])
        results = []
        pool.publish("", "q", "x", on_result=results.append)
        self.env.cr.commit()
        self.assertEqual(results, [False])
        self.assertEqual((refused.attempts, get_channel.call_count), (1, 1))

    def test_05_credentials_never_fall_back_to_guest(self):
        # [@ANCHOR: COMM_test_05_credentials_never_fall_back_to_guest]
        """Production (hams_prod) has no rabbitmq.* parameters, so the account comes from
        RMQ_USER/RMQ_PASS in odoo.service's environment; with neither source there is no account,
        never the factory guest/guest one."""
        # Tests [@ANCHOR: rabbitmq_resolve_credentials]
        env = {"RMQ_USER": "hams_rabbitmq", "RMQ_PASS": "from-env"}
        self.assertEqual(resolve_rabbitmq_credentials("param_user", "param_pass", env), ("param_user", "param_pass"))
        self.assertEqual(resolve_rabbitmq_credentials(None, None, env), ("hams_rabbitmq", "from-env"))
        self.assertEqual(resolve_rabbitmq_credentials("param_user", None, env), ("hams_rabbitmq", "from-env"))
        self.assertIsNone(resolve_rabbitmq_credentials(None, None, {}))
        self.assertIsNone(resolve_rabbitmq_credentials(None, None, {"RMQ_USER": "hams_rabbitmq"}))
