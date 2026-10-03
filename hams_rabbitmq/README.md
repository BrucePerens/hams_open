# Hams RabbitMQ

The `hams_rabbitmq` module provides a global RabbitMQ Connection Pool via an abstract model `hams_rabbitmq.pool`. This enables other modules to safely publish messages to a RabbitMQ broker using Odoo's environment.

## Developer API

### Abstract Model: `hams_rabbitmq.pool`

Developers can inherit or call the abstract model to publish messages:
`self.env['hams_rabbitmq.pool'].publish(exchange, routing_key, body, properties=None, on_result=None)` ([@ANCHOR: rabbitmq_publish]).

`publish()` always returns `True` meaning only "queued for after commit". To learn whether the real AMQP send succeeded, pass `on_result`, a callable invoked as `on_result(success: bool)` from the postcommit hook after the send is attempted; it runs after commit (open your own cursor if it must write) and its exceptions are logged, never raised.

A pooled connection the broker has dropped since it was last used (broker restart, heartbeat timeout, its user deleted) fails on first use, typically with `StreamLostError` ("Connection reset by peer"). The send then discards that connection ([@ANCHOR: rabbitmq_discard_stale_connection]) and retries exactly once on a freshly opened one, logging a warning. Only if the retry fails too does `on_result(False)` fire. A refusal on a live channel (e.g. `UnroutableError`) is not retried.

### Credentials & Security
The `_get_channel()` logic ([@ANCHOR: rabbitmq_get_channel]) dynamically relies on the `zero_sudo.security.utils` abstract model to fetch RabbitMQ credentials securely without hardcoding them in the source.

Which account it uses ([@ANCHOR: rabbitmq_resolve_credentials]): the `rabbitmq.user`/`rabbitmq.pass` system parameters when both are set, otherwise `RMQ_USER`/`RMQ_PASS` from the Odoo process's environment (`odoo.service` loads `/opt/hams/etc/rabbitmq.env`, the same file the RabbitMQ daemons read). With neither, it logs an error and does not connect. There is no fallback to RabbitMQ's factory `guest` account: production published as `guest` this way until 2026-10-03, which kept that account alive on the production broker.

### Post-Commit Hook Strategy
Message publishing should ideally be done within Odoo's `postcommit` hook to ensure messages are only sent if the database transaction successfully commits.
