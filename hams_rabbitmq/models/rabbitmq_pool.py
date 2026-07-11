import pika
import threading
import logging
import json
from odoo import models, api

_logger = logging.getLogger(__name__)

class RabbitMQPool(models.AbstractModel):
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
                    # In a real scenario, these would come from config parameters
                    credentials = pika.PlainCredentials('guest', 'guest')
                    parameters = pika.ConnectionParameters('localhost', 5672, '/', credentials)
                    self.__class__._connection = pika.BlockingConnection(parameters)
                    self.__class__._channel = self.__class__._connection.channel()
                except Exception as e:
                    _logger.error(f"Failed to connect to RabbitMQ: {e}")
                    return None
            elif not self._channel or self._channel.is_closed:
                try:
                    self.__class__._channel = self.__class__._connection.channel()
                except Exception as e:
                    _logger.error(f"Failed to create RabbitMQ channel: {e}")
                    return None
            
            return self._channel

    @api.model
    def publish(self, exchange, routing_key, body, properties=None):
        """
        Publishes a message using the global connection pool.
        """
        channel = self._get_channel()
        if not channel:
            _logger.error("Cannot publish message, no RabbitMQ channel available.")
            return False

        if isinstance(body, dict):
            body = json.dumps(body)

        try:
            with self._lock:
                channel.basic_publish(
                    exchange=exchange,
                    routing_key=routing_key,
                    body=body,
                    properties=properties or pika.BasicProperties(delivery_mode=2)
                )
            return True
        except Exception as e:
            _logger.error(f"Failed to publish message to RabbitMQ: {e}")
            # Force reconnect on next attempt
            self.__class__._connection = None
            return False
