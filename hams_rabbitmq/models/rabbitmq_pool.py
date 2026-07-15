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
    def publish(self, exchange, routing_key, body, properties=None):
        """
        Publishes a message using the global connection pool.
        """
        if isinstance(body, dict):
            body = json.dumps(body)

        def _do_publish():
            channel = self._get_channel()
            if not channel:
                _logger.error("Cannot publish message, no RabbitMQ channel available.")
                return False
            try:
                with self._lock:
                    channel.basic_publish(
                        exchange=exchange,
                        routing_key=routing_key,
                        body=body,
                        properties=properties or pika.BasicProperties(delivery_mode=2)
                    )
                return True
            except pika.exceptions.AMQPError:
                _logger.exception("Failed to publish message to RabbitMQ")
                # Force reconnect on next attempt
                self.__class__._connection = None
                return False

        self.env.cr.postcommit.add(_do_publish)
        return True
