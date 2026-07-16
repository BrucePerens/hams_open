# Hams RabbitMQ

The `hams_rabbitmq` module provides a global RabbitMQ Connection Pool via an abstract model `hams_rabbitmq.pool`. This enables other modules to safely publish messages to a RabbitMQ broker using Odoo's environment.

## Developer API

### Abstract Model: `hams_rabbitmq.pool`

Developers can inherit or call the abstract model to publish messages:
`self.env['hams_rabbitmq.pool'].publish(exchange, routing_key, body, properties=None)`

### Credentials & Security
The `_get_channel()` logic dynamically relies on the `zero_sudo.security.utils` abstract model to fetch RabbitMQ credentials securely without hardcoding them in the source.

### Post-Commit Hook Strategy
Message publishing should ideally be done within Odoo's `postcommit` hook to ensure messages are only sent if the database transaction successfully commits.
