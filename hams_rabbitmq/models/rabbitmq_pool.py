# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

import atexit
import pika
import threading
import time
import logging
import json
import os
from odoo import models, fields, api


_logger = logging.getLogger(__name__)

# Errors that mean the pooled connection or channel is dead rather than that the broker refused
# this one message: StreamLostError (socket reset, e.g. "Connection reset by peer" after the
# broker dropped the connection), ConnectionClosed/ConnectionClosedByBroker (broker restart,
# heartbeat timeout, the connection's user deleted), ChannelClosed and the wrong-state errors, and
# a raw socket ConnectionError (ConnectionResetError, BrokenPipeError) that pika did not wrap.
# publish() retries exactly once, on a freshly opened connection, after any of these. Not the
# other AMQPChannelError subclasses (UnroutableError, NackError): those are the broker answering
# about this message on a live channel, and resending would not change the answer.
STALE_CONNECTION_ERRORS = (
    pika.exceptions.AMQPConnectionError,
    pika.exceptions.ChannelClosed,
    pika.exceptions.ChannelWrongStateError,
    ConnectionError,
)


# AMQP heartbeat timeout asked of the broker. pika's BlockingConnection answers (and sends) heartbeats only
# while the process is inside a pika call, so a pooled connection left alone between publishes goes silent
# and the broker resets it ("missed heartbeats from client, timeout: 60s"). Production, 2026-10-15..19: 262
# such closes in three days from Odoo workers (the cron worker most), each costing the next publish four
# pika ERROR lines and a retry.
HEARTBEAT_SECONDS = 60

# Keepalive: while a pooled connection sits idle, a per-process daemon thread services it (pika's
# process_data_events(time_limit=0): answers the broker's heartbeat and sends ours when pika's timer says it is
# due) whenever the connection has been quiet this long. pika sends a heartbeat every HEARTBEAT_SECONDS / 2 (30 s)
# and the broker gives up after two silent intervals (60 s), so servicing every 15 s keeps a missed round to a
# fraction of the budget. Threads: see _start_keepalive_thread().
KEEPALIVE_SECONDS = 15

# Safety net: a pooled connection that has neither carried a message nor been serviced by the keepalive for this
# long (the keepalive thread died or never ran) is closed cleanly and replaced before use, rather than sent on
# after the broker has probably reset it.
IDLE_RECYCLE_SECONDS = 300


# [@ANCHOR: rabbitmq_resolve_credentials]
def resolve_rabbitmq_credentials(param_user, param_pass, environ):
    """
    The RabbitMQ account Odoo publishes as: the rabbitmq.user/rabbitmq.pass system parameters if
    both are set, otherwise RMQ_USER/RMQ_PASS from the environment (odoo.service loads
    /opt/hams/etc/rabbitmq.env, the same file every RabbitMQ daemon reads). Returns None when
    neither source gives both values.

    There is deliberately no "guest" fallback. Until 2026-10-03 this fell back to guest/guest, and
    production (hams_prod has no rabbitmq.* parameters) published as RabbitMQ's factory account
    for its whole life, which kept that account alive on the production broker.
    """
    if param_user and param_pass:
        return param_user, param_pass
    env_user = environ.get("RMQ_USER")
    env_pass = environ.get("RMQ_PASS")
    if env_user and env_pass:
        return env_user, env_pass
    return None


# One lock per process for the pooled connection, its channel and the keepalive thread. Module level (not created per
# registry class) so the fork hooks below can reach it.
_POOL_LOCK = threading.Lock()

# The keepalive thread of each pool class in this process (one registry builds one class, and the connection is a class
# attribute) and the Event that stops it: {pool_class: (thread, stop_event)}. Guarded by _POOL_LOCK. A thread never
# survives fork(), so a child starts its own (see _reset_after_fork).
_keepalive_threads = {}
_shutdown_registered = False


def _reset_after_fork():
    # [@ANCHOR: rabbitmq_reset_after_fork]

    # # Verified by [@ANCHOR: COMM_test_13_fork_neither_inherits_the_lock_locked_nor_touches_the_parents_connection]
    # Runs in the child just after fork(), where only the forking thread exists. The parent's keepalive threads are
    # gone and their Events are the parent's; forget them so the child starts its own the first time it opens a
    # connection. The pooled connection itself is left alone here: _retire_unusable_connection drops it by its
    # recorded pid, without closing the parent's socket.
    _keepalive_threads.clear()
    _POOL_LOCK.release()


# Hold the lock across fork() so the child never inherits it locked by a keepalive tick or publish in another thread
# (which would deadlock the child on its first publish). The forking thread releases it on both sides.
os.register_at_fork(
    before=_POOL_LOCK.acquire,
    after_in_parent=_POOL_LOCK.release,
    after_in_child=_reset_after_fork,
)


class RabbitMQPool(models.AbstractModel):
    name = fields.Char(string="Name", required=True)
    _name = "hams_rabbitmq.pool"
    _description = "Global RabbitMQ Connection Pool"

    # We use class variables for the singleton pattern across Odoo workers/threads
    _connection = None
    _channel = None
    # Which process opened the pooled connection, and when it last carried a message or was serviced by the keepalive
    # (time.monotonic()).
    _connection_pid = None
    _last_used = None
    _lock = _POOL_LOCK

    @api.model
    def _get_channel(self):
        # [@ANCHOR: rabbitmq_get_channel]

        # # Verified by [@ANCHOR: COMM_test_01_get_channel_connects_with_real_config] [@ANCHOR: COMM_test_04_get_channel_recreates_a_closed_channel_on_an_open_connection]
        with self._lock:
            self._retire_unusable_connection()
            if not self._connection or self._connection.is_closed:
                try:
                    utils = self.env['zero_sudo.security.utils']
                    credentials_pair = resolve_rabbitmq_credentials(
                        utils._get_system_param('rabbitmq.user'),
                        utils._get_system_param('rabbitmq.pass'),
                        os.environ,  # burn-ignore-env: RMQ_USER/RMQ_PASS from rabbitmq.env # Tested by [@ANCHOR: COMM_test_05_credentials_never_fall_back_to_guest]
                    )
                    if credentials_pair is None:
                        _logger.error(
                            "No RabbitMQ credentials: set RMQ_USER/RMQ_PASS (rabbitmq.env) or the "
                            "rabbitmq.user/rabbitmq.pass system parameters. Not publishing."
                        )
                        return None
                    mq_user, mq_pass = credentials_pair
                    mq_host = os.environ.get('RABBITMQ_HOST', 'rabbitmq')
                    mq_port = int(utils._get_system_param('rabbitmq.port') or 5672)
                    mq_vhost = utils._get_system_param('rabbitmq.vhost') or '/'
                    credentials = pika.PlainCredentials(mq_user, mq_pass)
                    parameters = pika.ConnectionParameters(
                        mq_host, mq_port, mq_vhost, credentials, heartbeat=HEARTBEAT_SECONDS,
                    )
                    self.__class__._connection = pika.BlockingConnection(parameters)
                    self.__class__._channel = self.__class__._connection.channel()
                    self.__class__._connection_pid = os.getpid()
                    self.__class__._last_used = time.monotonic()
                    self._start_keepalive_thread()
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

    def _start_keepalive_thread(self):
        # [@ANCHOR: rabbitmq_keepalive_thread]

        # # Verified by [@ANCHOR: COMM_test_11_an_idle_connection_survives_several_heartbeat_timeouts]
        # # Verified by [@ANCHOR: COMM_test_12_keepalive_thread_is_per_process_and_stops_cleanly] [@ANCHOR: COMM_test_13_fork_neither_inherits_the_lock_locked_nor_touches_the_parents_connection]
        """
        Called with the lock held, after a connection was opened: make sure this process has a keepalive thread.

        One daemon thread per process, started lazily (so a forked Odoo worker starts its own the first time it
        opens a connection; a thread does not survive fork). It exits by itself when the pool has no
        connection, so a process that does not publish runs nothing.

        Why a thread and not pika's SelectConnection: BlockingConnection is what the callers' channel API
        (basic_publish, basic_get, queue_declare) is built on, and it is not thread-safe; the thread therefore
        touches the connection only under the pool lock, the same lock every send takes. Only
        threading primitives are used (Thread, Event, Lock, no select/sleep of our own), so it works unchanged
        under Odoo's gevent process, where gevent monkey-patching turns the thread into a greenlet and pika's
        sockets into cooperative ones, and in the prefork workers (hams1 runs both), where it is a real thread
        that spends its life blocked in Event.wait().
        """
        global _shutdown_registered
        pool_class = type(self)
        existing = _keepalive_threads.get(pool_class)
        if existing is not None and existing[0].is_alive():
            return
        stop_event = threading.Event()
        thread = threading.Thread(  # burn-ignore-bounded-daemon-thread: one per pool class, started under the pool lock, ended by its Event or when the pool has no connection
            target=self._keepalive_loop,
            args=(pool_class, stop_event),
            name="hams_rabbitmq_keepalive",
            daemon=True,
        )
        _keepalive_threads[pool_class] = (thread, stop_event)
        thread.start()
        if not _shutdown_registered:
            _shutdown_registered = True
            atexit.register(self._shutdown_all)

    @staticmethod
    def _keepalive_loop(pool_class, stop_event):
        # [@ANCHOR: rabbitmq_keepalive_loop]

        # # Verified by [@ANCHOR: COMM_test_11_an_idle_connection_survives_several_heartbeat_timeouts]
        # Sleeps in Event.wait (no busy loop), ticks every KEEPALIVE_SECONDS, ends when a tick says the pool has no
        # connection to keep alive or when stop_event is set (shutdown()).
        while not stop_event.wait(KEEPALIVE_SECONDS):
            if not pool_class._keepalive_tick(stop_event):
                return

    @classmethod
    def _keepalive_tick(cls, stop_event):
        # [@ANCHOR: rabbitmq_keepalive_tick]

        # # Verified by [@ANCHOR: COMM_test_11_an_idle_connection_survives_several_heartbeat_timeouts] [@ANCHOR: COMM_test_12_keepalive_thread_is_per_process_and_stops_cleanly]
        """
        One keepalive round, under the pool lock. Returns False when the thread should end.

        Services the pooled connection if it has been quiet for KEEPALIVE_SECONDS (a publish that just ran has
        already given pika its chance). A connection another process opened is never touched. Any failure
        servicing it means the connection is dead: it is dropped quietly (the next publish opens a new one) and
        the thread ends.
        """
        with _POOL_LOCK:
            if stop_event.is_set():
                return False
            connection = cls._connection
            if connection is None or cls._connection_pid != os.getpid():
                cls._forget_keepalive(stop_event)
                return False
            if time.monotonic() - cls._last_used < KEEPALIVE_SECONDS:
                return True
            try:
                # A connection that is already closed is not serviced: pika raises ValueError("Timeout closed
                # before call") from its timer on one, which would only be noise here.
                alive = connection.is_open
                if alive:
                    connection.process_data_events(time_limit=0)
                    alive = connection.is_open
            except (pika.exceptions.AMQPError, OSError) as service_error:
                _logger.debug("RabbitMQ keepalive found the connection dead (%r); dropping it", service_error)
                alive = False
            except Exception:  # audit-ignore-catch-all: a daemon thread must not die silently; drop the connection and say so once
                _logger.warning("RabbitMQ keepalive failed unexpectedly; dropping the pooled connection", exc_info=True)
                alive = False
            if alive:
                cls._last_used = time.monotonic()
                return True
            cls._connection = cls._channel = cls._connection_pid = cls._last_used = None
            cls._forget_keepalive(stop_event)
            if connection.is_open:
                # (pika logs an ERROR for close() on a connection that is already closed)
                try:
                    connection.close()
                except (pika.exceptions.AMQPError, OSError) as close_error:
                    _logger.debug("Closing a dead RabbitMQ connection after the keepalive raised %r", close_error)
            return False

    @classmethod
    def _forget_keepalive(cls, stop_event):
        # [@ANCHOR: rabbitmq_forget_keepalive]

        # # Verified by [@ANCHOR: COMM_test_12_keepalive_thread_is_per_process_and_stops_cleanly]
        # Lock held. The thread that owns stop_event is about to end; unregister it unless a newer one replaced it.
        entry = _keepalive_threads.get(cls)
        if entry is not None and entry[1] is stop_event:
            del _keepalive_threads[cls]

    @staticmethod
    def _shutdown_all():
        # [@ANCHOR: rabbitmq_shutdown_all]

        # # Verified by [@ANCHOR: COMM_test_12_keepalive_thread_is_per_process_and_stops_cleanly]
        # Interpreter exit (atexit): stop every keepalive thread of this process and close its pooled connections.
        with _POOL_LOCK:
            pool_classes = list(_keepalive_threads)
        for pool_class in pool_classes:
            pool_class._shutdown()

    @classmethod
    def _shutdown(cls):
        # [@ANCHOR: rabbitmq_pool_shutdown]

        # # Verified by [@ANCHOR: COMM_test_12_keepalive_thread_is_per_process_and_stops_cleanly]
        """
        Stop this process's keepalive thread for the pool and close the pooled connection cleanly. A connection
        another process opened is left alone. Safe to call more than once; the next _get_channel() starts afresh.
        """
        with _POOL_LOCK:
            entry = _keepalive_threads.pop(cls, None)
            if entry is not None:
                entry[1].set()
            connection = cls._connection
            if connection is not None and cls._connection_pid == os.getpid():
                cls._connection = cls._channel = cls._connection_pid = cls._last_used = None
                try:
                    if connection.is_open:
                        connection.close()
                except (pika.exceptions.AMQPError, OSError) as close_error:
                    _logger.debug("Closing the RabbitMQ connection at shutdown raised %r", close_error)
        if entry is not None and entry[0] is not threading.current_thread():
            entry[0].join(timeout=5)

    def _retire_unusable_connection(self):
        # [@ANCHOR: rabbitmq_retire_idle_or_inherited_connection]

        # # Verified by [@ANCHOR: COMM_test_09_an_idle_connection_is_replaced_before_use_without_pika_errors]
        # # Verified by [@ANCHOR: COMM_test_10_a_connection_inherited_across_fork_is_never_used_or_closed]
        """
        Called with the lock held. Forget the pooled connection when it should not be used:

        * it was opened by another process (an Odoo worker forked after the master or its parent opened
          one): two processes must never write to one socket, and closing it would send the AMQP close to
          the broker for the other process too. The inherited objects are only dropped here; the child's
          copy of the socket closes when they are collected, and the other process keeps its own.
        * it has neither carried a message nor been serviced by the keepalive thread for longer than
          IDLE_RECYCLE_SECONDS: the broker is about to drop it, or already has, for want of heartbeats. It is closed cleanly now, while it is still healthy, so the next
          send opens a fresh one without a reset, a pika ERROR line or a retry.
        """
        cls = self.__class__
        connection = cls._connection
        if connection is None:
            return
        if cls._connection_pid is not None and cls._connection_pid != os.getpid():
            cls._connection = cls._channel = cls._connection_pid = cls._last_used = None
            return
        if cls._last_used is not None and time.monotonic() - cls._last_used > IDLE_RECYCLE_SECONDS:
            cls._connection = cls._channel = cls._connection_pid = cls._last_used = None
            try:
                connection.close()
            except (pika.exceptions.AMQPError, OSError) as close_error:
                _logger.debug("Closing an idle RabbitMQ connection raised %r", close_error)

    @api.model
    def _discard_connection(self, dead_channel):
        # [@ANCHOR: rabbitmq_discard_stale_connection]

        # # Verified by [@ANCHOR: COMM_test_07_publish_retries_once_on_a_fresh_connection_after_a_stale_one]
        # # Verified by [@ANCHOR: COMM_test_08_publish_reports_failure_when_the_retry_also_fails]
        """
        Drop the pooled connection that ``dead_channel`` came from, so the next _get_channel()
        opens a fresh one. Only if it is still the pooled channel: another thread may already have
        replaced it, and that newer connection must not be thrown away. Closing a connection the
        broker already dropped normally raises; that is expected and only logged at debug level.
        """
        with self._lock:
            if dead_channel is not self.__class__._channel:
                return
            dead_connection = self.__class__._connection
            self.__class__._connection = None
            self.__class__._channel = None
            self.__class__._connection_pid = None
            self.__class__._last_used = None
            if dead_connection is not None:
                try:
                    dead_connection.close()
                except (pika.exceptions.AMQPError, OSError) as close_error:
                    _logger.debug("Closing a dead RabbitMQ connection raised %r", close_error)

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

        A pooled connection the broker has dropped since its last use (broker
        restart, heartbeat timeout, its user deleted) fails on first use with
        one of STALE_CONNECTION_ERRORS. The send then discards that
        connection and retries exactly once on a freshly opened one; only if
        that also fails is the publish reported as failed.
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

        def _obtain_channel():
            try:
                return self._get_channel()
            except Exception:  # audit-ignore-catch-all
                _logger.exception(
                    "Unexpected error obtaining RabbitMQ channel "
                    "(exchange=%r, routing_key=%r)", exchange, routing_key,
                )
                return None

        def _send(channel):
            with self._lock:
                channel.basic_publish(
                    exchange=exchange,
                    routing_key=routing_key,
                    body=body,
                    properties=properties or pika.BasicProperties(delivery_mode=2)
                )
                self.__class__._last_used = time.monotonic()

        def _fail_no_channel():
            _logger.error(
                "Cannot publish message, no RabbitMQ channel available "
                "(exchange=%r, routing_key=%r).", exchange, routing_key
            )
            _report(False)
            return False

        def _do_publish():
            # publish() already returned True to its caller before this
            # postcommit callback runs, so the return value cannot carry the
            # outcome. Callers that need it pass on_result (see publish()'s
            # docstring); the failure branches also log exchange/routing_key
            # as the trail for reconciling a lost message.
            channel = _obtain_channel()
            if not channel:
                return _fail_no_channel()
            try:
                _send(channel)
            except STALE_CONNECTION_ERRORS as stale_error:
                # The pooled connection was dead before we used it (production,
                # 2026-10-03: StreamLostError after the broker dropped Odoo's
                # connection, and backup job 145 was lost). Retry once, fresh.
                _logger.warning(
                    "RabbitMQ connection was dead (%r); discarding it and retrying once on a "
                    "fresh connection (exchange=%r, routing_key=%r)",
                    stale_error, exchange, routing_key,
                )
                self._discard_connection(channel)
                channel = _obtain_channel()
                if not channel:
                    return _fail_no_channel()
                try:
                    _send(channel)
                except (pika.exceptions.AMQPError, ConnectionError):
                    _logger.exception(
                        "Failed to publish message to RabbitMQ on a fresh connection after a "
                        "stale one (exchange=%r, routing_key=%r)", exchange, routing_key,
                    )
                    self._discard_connection(channel)
                    _report(False)
                    return False
            except pika.exceptions.AMQPError:
                _logger.exception(
                    "Failed to publish message to RabbitMQ (exchange=%r, routing_key=%r)",
                    exchange, routing_key,
                )
                # Force reconnect on next attempt
                self._discard_connection(channel)
                _report(False)
                return False
            _report(True)
            return True

        self.env.cr.postcommit.add(_do_publish)
        return True
