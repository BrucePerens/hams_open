# Hams RabbitMQ

The `hams_rabbitmq` module provides a global RabbitMQ Connection Pool via an abstract model `hams_rabbitmq.pool`. This enables other modules to safely publish messages to a RabbitMQ broker using Odoo's environment.

"Pool" here means one shared connection, not a set of them: the model keeps a single RabbitMQ connection and a single channel as class attributes, guarded by a lock, and every caller reuses them. A closed connection is reopened, and a closed channel is replaced on the still-open connection, on the next use.

## Developer API

### Abstract Model: `hams_rabbitmq.pool`

Developers can inherit or call the abstract model to publish messages:
`self.env['hams_rabbitmq.pool'].publish(exchange, routing_key, body, properties=None, on_result=None)` ([@ANCHOR: rabbitmq_publish]).

`publish()` always returns `True` meaning only "queued for after commit". To learn whether the real AMQP (Advanced Message Queuing Protocol, the wire protocol RabbitMQ speaks) send succeeded, pass `on_result`, a callable invoked as `on_result(success: bool)` from the postcommit hook after the send is attempted; it runs after commit (open your own cursor if it must write) and its exceptions are logged, never raised.

A `dict` body is serialized to JSON before sending; any other body is passed through unchanged. If `properties` is omitted, the message is sent as persistent (`delivery_mode=2`). If no channel can be obtained, or `basic_publish` raises an AMQP error, the message is dropped, not retried: the failure is logged with its exchange and routing key, `on_result(False)` is called, and after a broker error the connection is discarded so the next publish reconnects.

### Credentials & Security
The `_get_channel()` logic ([@ANCHOR: rabbitmq_get_channel]) dynamically relies on the `zero_sudo.security.utils` abstract model to fetch RabbitMQ credentials at runtime instead of hardcoding them in the source; the only connection values written in the source are the fallbacks below. The user, password, port and virtual host are the Odoo system parameters `rabbitmq.user`, `rabbitmq.pass`, `rabbitmq.port` and `rabbitmq.vhost`, read through `zero_sudo`'s whitelisted `_get_system_param()`. If a parameter is unset, the code falls back to RabbitMQ's stock defaults: `guest` / `guest`, port `5672`, virtual host `/`. The broker host is not a system parameter: it comes from the `RABBITMQ_HOST` environment variable, default `rabbitmq`. If the connection or channel cannot be opened, the error is logged and `_get_channel()` returns `None`.

### Post-Commit Hook Strategy
Odoo's `postcommit` hook is the list of callbacks (`cr.postcommit`) that the database cursor runs only after its transaction has committed. `publish()` registers the actual send there itself, so callers simply call `publish()` inside their normal transaction: the message is sent only if that transaction commits, and never if it rolls back. Callers do not need to register their own `postcommit` callback.
