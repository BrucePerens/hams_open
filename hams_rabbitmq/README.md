# Hams RabbitMQ

The `hams_rabbitmq` module provides a global RabbitMQ Connection Pool via an abstract model `hams_rabbitmq.pool`. This enables other modules to safely publish messages to a RabbitMQ broker using Odoo's environment.

"Pool" here means one shared connection, not a set of them: the model keeps a single RabbitMQ connection and a single channel as class attributes, guarded by a lock, and every caller reuses them. A closed connection is reopened, and a closed channel is replaced on the still-open connection, on the next use.

## Developer API

### Abstract Model: `hams_rabbitmq.pool`

Developers can inherit or call the abstract model to publish messages:
`self.env['hams_rabbitmq.pool'].publish(exchange, routing_key, body, properties=None, on_result=None)` (`rabbitmq_publish`).

`publish()` always returns `True` meaning only "queued for after commit". To learn whether the real AMQP (Advanced Message Queuing Protocol, the wire protocol RabbitMQ speaks) send succeeded, pass `on_result`, a callable invoked as `on_result(success: bool)` from the postcommit hook after the send is attempted; it runs after commit (open your own cursor if it must write) and its exceptions are logged, never raised.

A `dict` body is serialized to JSON before sending; any other body is passed through unchanged. If `properties` is omitted, the message is sent as persistent (`delivery_mode=2`). If no channel can be obtained, or `basic_publish` raises an AMQP error, the message is dropped: the failure is logged with its exchange and routing key, `on_result(False)` is called, and after a broker error the connection is discarded so the next publish reconnects. The one exception is a dead pooled connection, which is retried once as described below.

A pooled connection the broker has dropped since it was last used (broker restart, heartbeat timeout, its user deleted) fails on first use, typically with `StreamLostError` ("Connection reset by peer"). The send then discards that connection (`rabbitmq_discard_stale_connection`) and retries exactly once on a freshly opened one, logging a warning. Only if the retry fails too does `on_result(False)` fire. A refusal on a live channel (e.g. `UnroutableError`) is not retried.

### Keepalive, idle and inherited connections
pika's `BlockingConnection` answers the broker's heartbeats only while the process is inside a pika call, so a pooled connection that sits idle between publishes goes silent and the broker resets it ("missed heartbeats from client", heartbeat timeout 60 s; 262 such closes in three days in production before the keepalive).

**Keepalive** (`rabbitmq_keepalive_thread`). When the pool opens a connection it starts, lazily, one daemon thread per process (`hams_rabbitmq_keepalive`). The thread body is a loop that waits on a stop `Event` with a `KEEPALIVE_SECONDS` timeout and returns as soon as a tick reports there is nothing left to keep alive (`rabbitmq_keepalive_loop`). Every `KEEPALIVE_SECONDS` (15) it takes the pool lock and, if the connection has been quiet that long, calls `process_data_events(time_limit=0)` (`rabbitmq_keepalive_tick`), which answers the broker's heartbeats and sends ours (pika's timer sends one every 30 s; the broker gives up after two silent intervals). The thread sleeps in `Event.wait` (no polling loop), touches the connection only under the same lock every send takes (BlockingConnection is not thread-safe), never touches a connection another process opened, and ends by itself when the pool has no connection or the connection is found dead (it is dropped quietly and the next publish opens a new one). Before a thread ends it unregisters itself from the per-process thread table, unless a newer thread has already replaced it, so a stale thread can never remove its successor's entry (`rabbitmq_forget_keepalive`). A thread does not survive `fork`, so a forked Odoo worker starts its own the first time it opens a connection; fork hooks hold the pool lock across `fork` so the child never inherits it locked (`rabbitmq_reset_after_fork`). At interpreter exit an `atexit` hook stops every keepalive thread this process started and closes its pooled connections (`rabbitmq_shutdown_all`); per pool class that is the clean shutdown, which sets the thread's stop event, closes the connection this process opened, and joins the thread (`rabbitmq_pool_shutdown`).

Why a thread rather than pika's `SelectConnection`: the callers' API (`basic_publish`, `basic_get`, `queue_declare`) is the blocking one, and a second, asynchronous connection class would mean rewriting every caller. The thread uses only `threading` primitives, so it works unchanged in Odoo's prefork workers (a real thread) and in its gevent process (gevent monkey-patching makes it a greenlet and pika's sockets cooperative); hams1 runs both.

**Safety net** (`rabbitmq_retire_idle_or_inherited_connection`). Before using the pooled connection, `_get_channel()` closes it cleanly and opens a fresh one when it has neither carried a message nor been serviced by the keepalive for `IDLE_RECYCLE_SECONDS` (300; the keepalive thread died or never ran). It also drops, without closing, a connection that another process opened (the pool's class attributes inherited through `fork`): two processes must not share one socket. The retry-once described above stays as the safety net for a broker restart.

### Credentials & Security
The `_get_channel()` logic (`rabbitmq_get_channel`) dynamically relies on the `zero_sudo.security.utils` abstract model to fetch RabbitMQ credentials at runtime instead of hardcoding them in the source; the only connection values written in the source are the fallbacks below. The user, password, port and virtual host come from the Odoo system parameters `rabbitmq.user`, `rabbitmq.pass`, `rabbitmq.port` and `rabbitmq.vhost`, read through `zero_sudo`'s whitelisted `_get_system_param()`. If `rabbitmq.port` is unset the code uses `5672`, and if `rabbitmq.vhost` is unset it uses `/`. The user and password fall back to the environment, as described next. The broker host is not a system parameter: it comes from the `RABBITMQ_HOST` environment variable, default `rabbitmq`. If the connection or channel cannot be opened, the error is logged and `_get_channel()` returns `None`.

Which account it uses (`rabbitmq_resolve_credentials`): the `rabbitmq.user`/`rabbitmq.pass` system parameters when both are set, otherwise `RMQ_USER`/`RMQ_PASS` from the Odoo process's environment (`odoo.service` loads `/opt/hams/etc/rabbitmq.env`, the same file the RabbitMQ daemons read). With neither, it logs an error and does not connect. There is no fallback to RabbitMQ's factory `guest` account: production published as `guest` this way until 2026-10-03, which kept that account alive on the production broker.

### Post-Commit Hook Strategy
Odoo's `postcommit` hook is the list of callbacks (`cr.postcommit`) that the database cursor runs only after its transaction has committed. `publish()` registers the actual send there itself, so callers simply call `publish()` inside their normal transaction: the message is sent only if that transaction commits, and never if it rolls back. Callers do not need to register their own `postcommit` callback.

## Anchor reference

One entry per anchor named above, read from `models/rabbitmq_pool.py`. The pool keeps its state in class attributes (`_connection`, `_channel`, `_connection_pid`, `_last_used`) guarded by the single process-wide lock `_POOL_LOCK`; every function below that touches them says whether it expects the lock to be held.

### `rabbitmq_publish` [@ANCHOR: rabbitmq_publish]

`hams_rabbitmq.pool.publish(self, exchange, routing_key, body, properties=None, on_result=None)` (`@api.private`, `@api.model`; not callable over RPC). It queues one AMQP message for sending after the current database transaction commits, by adding a closure to `self.env.cr.postcommit`.

| Argument | Type | Meaning |
|---|---|---|
| `exchange` | str | Exchange name (empty string for the default exchange). |
| `routing_key` | str | Routing key, or queue name on the default exchange. |
| `body` | dict, str or bytes | A `dict` is serialised with `json.dumps`; anything else is sent as given. |
| `properties` | `pika.BasicProperties` or None | Defaults to persistent delivery (`delivery_mode=2`). |
| `on_result` | callable or None | Called as `on_result(success: bool)` after the real send was attempted; it runs after commit, so it must open its own cursor to write to the database. An exception from it is logged and never reaches the commit path. |

**Returns:** always `True`, meaning only "queued". If the transaction rolls back nothing is sent. **Post-commit work:** obtains the pooled channel (`rabbitmq_get_channel`), sends under the pool lock, and on a stale-connection error (`STALE_CONNECTION_ERRORS`) discards the connection and retries exactly once on a fresh one (`rabbitmq_discard_stale_connection`); other AMQP errors, no credentials or no channel are logged with exchange and routing key and reported as `on_result(False)`. The send blocks on the network inside the commit path. Not idempotent: a retry after an ambiguous failure can duplicate a message. Tests: `test_02_publish_delivers_a_real_message_after_commit`, `test_03_publish_serializes_dict_bodies_to_json`, `test_07_publish_retries_once_on_a_fresh_connection_after_a_stale_one`, `test_08_publish_reports_failure_when_the_retry_also_fails`.

### `rabbitmq_discard_stale_connection` [@ANCHOR: rabbitmq_discard_stale_connection]

`RabbitMQPool._discard_connection(self, dead_channel)` (`@api.model`). Drops the pooled connection that `dead_channel` belongs to so that the next `_get_channel()` opens a new one. Under the pool lock it does nothing unless `dead_channel` is still the pooled channel, because another thread may already have replaced it and that newer connection must not be thrown away. Otherwise it clears the class attributes and tries to close the old connection; closing a connection the broker already dropped normally raises, and that `AMQPError` or `OSError` is only logged at debug level. Called by `publish` after a stale-connection error and after a failed retry. Returns nothing. Tests: `test_07_publish_retries_once_on_a_fresh_connection_after_a_stale_one`, `test_08_publish_reports_failure_when_the_retry_also_fails`.

### `rabbitmq_keepalive_thread` [@ANCHOR: rabbitmq_keepalive_thread]

`RabbitMQPool._start_keepalive_thread(self)`, called by `_get_channel` with the pool lock held right after a connection has been opened. If this process already has a live keepalive thread for the pool class (table `_keepalive_threads`, `{pool_class: (thread, stop_event)}`), it returns. Otherwise it creates a stop `threading.Event`, starts one daemon thread named `hams_rabbitmq_keepalive` running `_keepalive_loop`, records it, and on the first thread of the process registers `_shutdown_all` with `atexit`. It is lazy so that a forked worker starts its own thread (threads do not survive `fork`). It uses only `threading` primitives, so it also works under gevent. Returns nothing. Tests: `test_11_an_idle_connection_survives_several_heartbeat_timeouts`, `test_12_keepalive_thread_is_per_process_and_stops_cleanly`, `test_13_fork_neither_inherits_the_lock_locked_nor_touches_the_parents_connection`.

### `rabbitmq_keepalive_loop` [@ANCHOR: rabbitmq_keepalive_loop]

`RabbitMQPool._keepalive_loop(pool_class, stop_event)` (static method), the body of the keepalive thread. It runs `while not stop_event.wait(KEEPALIVE_SECONDS)` (15 seconds), so it sleeps blocked in `Event.wait` and never polls, and on each wake calls `pool_class._keepalive_tick(stop_event)`. It returns, ending the thread, when `stop_event` is set (by `_shutdown`) or when a tick returns False (the pool has no connection to keep alive, or the connection died). It takes no lock itself and handles no exceptions itself; `_keepalive_tick` catches the connection errors. Test: `test_11_an_idle_connection_survives_several_heartbeat_timeouts`.

### `rabbitmq_keepalive_tick` [@ANCHOR: rabbitmq_keepalive_tick]

`RabbitMQPool._keepalive_tick(cls, stop_event)` (classmethod), one keepalive round under the process-wide pool lock. Returns True to keep the thread running and False when it should end. In order: returns False if `stop_event` is set; if there is no pooled connection, or it was opened by another process (`_connection_pid != os.getpid()`), unregisters the thread (`_forget_keepalive`) and returns False; returns True without work if the connection carried a message or was serviced within `KEEPALIVE_SECONDS`; otherwise, if the connection is open, calls `connection.process_data_events(time_limit=0)`, which answers the broker's heartbeat and sends ours. Success updates `_last_used` and returns True. An AMQP or OS error is logged at debug level, any other exception at warning with a traceback, and either way the connection is dropped (class attributes cleared, `close()` attempted if still open), the thread unregistered, and False returned. The next publish then opens a new connection. Tests: `test_11_an_idle_connection_survives_several_heartbeat_timeouts`, `test_12_keepalive_thread_is_per_process_and_stops_cleanly`.

### `rabbitmq_forget_keepalive` [@ANCHOR: rabbitmq_forget_keepalive]

`RabbitMQPool._forget_keepalive(cls, stop_event)` (classmethod), called with the pool lock held by the thread that owns `stop_event` just before it ends. It looks up the thread entry for `cls` in `_keepalive_threads` and deletes it only if the stored event is the same object as `stop_event`. A thread that was already replaced by a newer one therefore cannot remove its successor's entry, which would otherwise leave the new thread untracked and let a third one be started. No return value, no I/O, nothing raised. Test: `test_12_keepalive_thread_is_per_process_and_stops_cleanly`.

### `rabbitmq_reset_after_fork` [@ANCHOR: rabbitmq_reset_after_fork]

`_reset_after_fork()` at module level in `models/rabbitmq_pool.py`, registered with `os.register_at_fork(before=_POOL_LOCK.acquire, after_in_parent=_POOL_LOCK.release, after_in_child=_reset_after_fork)`. The lock is held across `fork()` so the child can never inherit it locked by a keepalive tick or publish in another thread, which would deadlock its first publish. In the child, where only the forking thread exists, this function clears `_keepalive_threads` (the parent's threads and events are gone) and releases the lock. It does not touch the pooled connection: `_retire_unusable_connection` drops it later by its recorded pid without closing the parent's socket. No arguments, no return value. Test: `test_13_fork_neither_inherits_the_lock_locked_nor_touches_the_parents_connection`.

### `rabbitmq_shutdown_all` [@ANCHOR: rabbitmq_shutdown_all]

`RabbitMQPool._shutdown_all()` (static method), registered with `atexit` when the first keepalive thread of the process starts. At interpreter exit it takes the pool lock only long enough to copy the list of pool classes in `_keepalive_threads`, then calls `_shutdown()` on each one outside the lock. The effect is that every keepalive thread this process started is stopped and the connections it opened are closed cleanly, so the broker does not log abrupt client disconnects. No arguments, no return value. Test: `test_12_keepalive_thread_is_per_process_and_stops_cleanly`.

### `rabbitmq_pool_shutdown` [@ANCHOR: rabbitmq_pool_shutdown]

`RabbitMQPool._shutdown(cls)` (classmethod). Under the pool lock it removes this class's thread entry and sets its stop event, then, if the pooled connection was opened by this process, clears the class attributes (`_connection`, `_channel`, `_connection_pid`, `_last_used`) and closes the connection if it is open; an AMQP or OS error on close is logged at debug level. A connection opened by another process is left alone. After releasing the lock it joins the thread with a 5-second timeout unless it is the current thread. Safe to call repeatedly; the next `_get_channel()` starts afresh. Called by `_shutdown_all` and by tests. Returns nothing. Test: `test_12_keepalive_thread_is_per_process_and_stops_cleanly`.

### `rabbitmq_retire_idle_or_inherited_connection` [@ANCHOR: rabbitmq_retire_idle_or_inherited_connection]

`RabbitMQPool._retire_unusable_connection(self)`, called by `_get_channel` with the lock held, before the pooled connection is used. It forgets the connection (clears the four class attributes) in two cases. First, it was opened by another process (recorded `_connection_pid` differs from `os.getpid()`, the case of a forked Odoo worker): it is only dropped, never closed, because two processes must not share one socket and closing would send the AMQP close for the other process too. Second, it has carried no message and received no keepalive service for more than `IDLE_RECYCLE_SECONDS` (300): it is closed cleanly while still healthy (a close error is logged at debug level), so the next send opens a fresh connection without a broker reset, a pika error line or a retry. Returns nothing. Tests: `test_09_an_idle_connection_is_replaced_before_use_without_pika_errors`, `test_10_a_connection_inherited_across_fork_is_never_used_or_closed`.

### `rabbitmq_get_channel` [@ANCHOR: rabbitmq_get_channel]

`RabbitMQPool._get_channel(self)` (`@api.model`). Returns a usable `pika` channel for the pooled connection, or `None` when none can be made (never raises for AMQP errors). Under the pool lock it first calls `_retire_unusable_connection`. With no open connection it resolves credentials (`rabbitmq_resolve_credentials`; logs an error and returns `None` if there are none), reads host from the environment `RABBITMQ_HOST` (default `rabbitmq`), port from the system parameter `rabbitmq.port` (default 5672) and virtual host from `rabbitmq.vhost` (default `/`) through `zero_sudo.security.utils._get_system_param`, opens `pika.BlockingConnection` with `heartbeat=HEARTBEAT_SECONDS` (60), opens a channel, records the pid and `time.monotonic()` and starts the keepalive thread. With an open connection but a closed or missing channel it opens a new channel. `AMQPError` on connect or channel creation is logged and gives `None`. Blocks on the network while connecting. Tests: `test_01_get_channel_connects_with_real_config`, `test_04_get_channel_recreates_a_closed_channel_on_an_open_connection`.

### `rabbitmq_resolve_credentials` [@ANCHOR: rabbitmq_resolve_credentials]

`resolve_rabbitmq_credentials(param_user, param_pass, environ)`, a pure module-level function. Returns the pair `(user, password)` the pool connects as, or `None`. The `rabbitmq.user` and `rabbitmq.pass` system-parameter values win when both are truthy; otherwise `RMQ_USER` and `RMQ_PASS` from the mapping `environ` (the Odoo process loads them from `/opt/hams/etc/rabbitmq.env`) when both are set; otherwise `None`. There is deliberately no fallback to the broker's factory `guest` account: until 2026-10-03 there was one, and production published as `guest` for its whole life, keeping that account alive. A caller that gets `None` must not publish. Takes no locks and has no side effects. Test: `test_05_credentials_never_fall_back_to_guest`.
