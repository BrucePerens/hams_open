# This software is distributed under the terms of the Affero General Public License (AGPL-3).

import pika
import threading
import logging
import json
import os
from odoo import models, fields, api


_logger = logging.getLogger(__name__)


class RabbitMQPool(models.AbstractModel):
    name = fields.Char(string="Name", required=True)
    _name = "hams_rabbitmq.pool"
    _description = "Global RabbitMQ Connection Pool"

    # We use class variables for the singleton pattern across Odoo workers/threads
    _connection = None
    _channel = None
    _lock = threading.Lock()

    @api.model
    def _get_channel(self):
        # [@ANCHOR: rabbitmq_get_channel]

        # # Verified by [@ANCHOR: COMM_test_01_get_channel_connects_with_real_config] [@ANCHOR: COMM_test_04_get_channel_recreates_a_closed_channel_on_an_open_connection]
        with self._lock:
            if not self._connection or self._connection.is_closed:
                try:
                    utils = self.env['zero_sudo.security.utils']
                    mq_user = utils._get_system_param('rabbitmq.user') or 'guest'
                    mq_pass = utils._get_system_param('rabbitmq.pass') or 'guest'
                    mq_host = os.environ.get('RABBITMQ_HOST', 'rabbitmq')
                    mq_port = int(utils._get_system_param('rabbitmq.port') or 5672)
                    mq_vhost = utils._get_system_param('rabbitmq.vhost') or '/'
                    credentials = pika.PlainCredentials(mq_user, mq_pass)
                    parameters = pika.ConnectionParameters(mq_host, mq_port, mq_vhost, credentials)
                    self.__class__._connection = pika.BlockingConnection(parameters)
                    self.__class__._channel = self.__class__._connection.channel()
                except pika.exceptions.AMQPError:
                    _logger.exception("Failed to connect to RabbitMQ")
                    return None
            elif not self._channel or self._channel.is_closed:
                try:
                    self.__class__._channel = self.__class__._connection.channel()
                except pika.exceptions.AMQPError:
                    _logger.exception("Failed to create RabbitMQ channel")
                    return None
            
            return self._channel

    @api.model
    def publish(self, exchange, routing_key, body, properties=None, on_result=None):
        # [@ANCHOR: rabbitmq_publish]

        # # Verified by [@ANCHOR: COMM_test_02_publish_delivers_a_real_message_after_commit] [@ANCHOR: COMM_test_03_publish_serializes_dict_bodies_to_json]
        """
        Publishes a message using the global connection pool.

        The actual send is deferred to cr.postcommit, so the return value
        (always True) only means "queued for after commit", never "delivered".
        To learn the real outcome, pass ``on_result``: a callable invoked as
        ``on_result(success: bool)`` from the postcommit hook once the real
        AMQP publish has been attempted (True only if basic_publish returned
        without error). It runs after the transaction has committed, so it
        must open its own cursor/transaction if it needs to write to the
        database. An exception raised by ``on_result`` is logged and never
        propagates into the commit path.
        """
        if isinstance(body, dict):
            body = json.dumps(body)

        def _report(success):
            if on_result is None:
                return
            try:
                on_result(success)
            except Exception:  # audit-ignore-catch-all: caller callback must never break the commit path
                _logger.exception(
                    "on_result callback failed for RabbitMQ publish "
                    "(exchange=%r, routing_key=%r)", exchange, routing_key,
                )

        def _do_publish():
            # publish() already returned True to its caller before this
            # postcommit callback runs, so the return value cannot carry the
            # outcome. Callers that need it pass on_result (see publish()'s
            # docstring); the failure branches also log exchange/routing_key
            # as the trail for reconciling a lost message.
            try:
                channel = self._get_channel()
            except Exception:  # audit-ignore-catch-all
                _logger.exception(
                    "Unexpected error obtaining RabbitMQ channel "
                    "(exchange=%r, routing_key=%r)", exchange, routing_key,
                )
                channel = None
            if not channel:
                _logger.error(
                    "Cannot publish message, no RabbitMQ channel available "
                    "(exchange=%r, routing_key=%r).", exchange, routing_key
                )
                _report(False)
                return False
            try:
                with self._lock:
                    channel.basic_publish(
                        exchange=exchange,
                        routing_key=routing_key,
                        body=body,
                        properties=properties or pika.BasicProperties(delivery_mode=2)
                    )
            except pika.exceptions.AMQPError:
                _logger.exception(
                    "Failed to publish message to RabbitMQ (exchange=%r, routing_key=%r)",
                    exchange, routing_key,
                )
                # Force reconnect on next attempt
                self.__class__._connection = None
                _report(False)
                return False
            _report(True)
            return True

        self.env.cr.postcommit.add(_do_publish)
        return True
