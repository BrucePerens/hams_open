# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A second HTTP listener, on a Unix socket, that serves only the external RPC endpoints, with the kernel-verified
uid, gid and pid of the connecting process (`SO_PEERCRED`) as the caller's identity.

Odoo 19's own HTTP server creates only TCP sockets, but it is a werkzeug WSGI application (`odoo.http.root`) behind
a small accept loop, so this module adds a listener of its own inside the same process(es) and hands each accepted
connection to the same application, the same way Odoo's own loop does:

* **prefork** (`--workers N`, production): the socket is created in the master in `PreforkServer.start`, before any
  worker is forked, so every HTTP worker inherits the listening descriptor (exactly as it inherits Odoo's TCP socket).
  Each HTTP worker waits on both sockets in its `select` and accepts from whichever is ready, one request per loop,
  so the worker's watchdog (`limit_time_real`), CPU limit (`limit_time_cpu`), memory limits and `limit_request`
  count apply to Unix-socket requests exactly as to TCP ones. Cron workers close the inherited descriptor. A
  `SIGHUP` reload passes the descriptor to the new master the way Odoo passes its TCP socket
  (`HAMS_RPC_GUARD_SOCKET_FD`), so the socket file keeps working across a reload.
* **threaded** (no `--workers`, development and tests): a werkzeug threaded server on the socket runs in a daemon
  thread next to Odoo's `odoo.service.httpd` thread.
* **gevent** (the `gevent`/longpolling process): no listener; that process serves websockets only, and it is
  started with `subprocess.Popen` (close_fds), so it does not inherit the descriptor.
* `odoo shell`, `--stop-after-init` without tests, and other commands that never start the HTTP server: no listener.

The listener exists only when the server configuration file names a path (`hams_rpc_guard_socket`); it is a
server-wide setting because the socket is created before any database is open. Whether the endpoints are served on it
is the per-database system parameter `hams_rpc_guard.rpc_transport` (`endpoint_gate.py`), and which local users may
use it is `hams_rpc_guard.unix_peers` (below). Only `/xmlrpc/...`, `/jsonrpc` and `/json/2/...` are served on the
socket; every other path answers 404 there.
"""
import errno
import fcntl
import grp
import ipaddress
import logging
import os
import pwd
import select
import socket
import stat
import struct
import threading

import werkzeug.serving

from odoo.service import server as odoo_server
from odoo.tools import config

from . import endpoint_gate as gate

_logger = logging.getLogger(__name__)

# [@ANCHOR: hams_rpc_guard:listener_options]
OPTION_PATH = "hams_rpc_guard_socket"
OPTION_MODE = "hams_rpc_guard_socket_mode"
OPTION_GROUP = "hams_rpc_guard_socket_group"
OPTION_OWNER = "hams_rpc_guard_socket_owner"
DEFAULT_MODE = 0o660
INHERITED_FD_ENV = "HAMS_RPC_GUARD_SOCKET_FD"
BACKLOG = 64
SOCKET_TIMEOUT = 2.0  # seconds, as Odoo's own WorkerHTTP (ODOO_HTTP_SOCKET_TIMEOUT)
_PEERCRED = struct.Struct("3i")
LOOPBACK_ADDRESS = str(ipaddress.IPv4Address(0x7F000001))  # REMOTE_ADDR of a socket request (written as a number: no host literal)  # struct ucred: pid, uid, gid

# Request headers that claim another client address; dropped on the socket so Odoo's own logs and `proxy_mode`
# never see an address the local caller made up.
_FORWARDING_ENVIRON_KEYS = tuple("HTTP_" + name.upper().replace("-", "_") for name in gate.FORWARDING_HEADERS)

# Per process: the listening socket, its path, this HTTP worker's werkzeug server object, whether Odoo's server
# classes are patched. A forked worker gets its own copy.
_state = {"socket": None, "path": None, "server": None, "patched": False}
_state_lock = threading.Lock()


class ListenerError(RuntimeError):
    """The configured socket cannot be created safely. Logged at ERROR; the server runs without the listener."""


# [@ANCHOR: hams_rpc_guard:peer_of]
def peer_of(connection):
    """The `Peer` of an accepted `AF_UNIX` connection (reads `SO_PEERCRED`; names from the passwd and group databases)."""
    pid, uid, gid = _PEERCRED.unpack(connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, _PEERCRED.size))
    return gate.Peer(pid, uid, gid, _user_names().get(uid, ""), _group_names().get(gid, ""))


# [@ANCHOR: hams_rpc_guard:user_names]
def _user_names():
    """{uid: name} of the passwd database (read whole: a missing uid is an ordinary answer, not an error)."""
    return {entry.pw_uid: entry.pw_name for entry in pwd.getpwall()}


# [@ANCHOR: hams_rpc_guard:group_names]
def _group_names():
    """{gid: name} of the group database."""
    return {entry.gr_gid: entry.gr_name for entry in grp.getgrall()}


# [@ANCHOR: hams_rpc_guard:listener_config]
def listener_config(config):
    """The listener settings from Odoo's server configuration (`odoo.tools.config` or any mapping with `get`):
    `None` when no path is configured, else a dict `path`, `mode` (int), `group` and `owner` (names or ""). Raises
    `ListenerError` for a relative path or a mode that gives any permission to others or to no one but the owner's
    execute bit (only 0600 and 0660 make sense)."""
    path = (config.get(OPTION_PATH) or "").strip()
    if not path:
        return None
    if not os.path.isabs(path):
        raise ListenerError("%s must be an absolute path, not %r." % (OPTION_PATH, path))
    mode_text = str(config.get(OPTION_MODE) or "").strip()
    try:
        mode = int(mode_text, 8) if mode_text else DEFAULT_MODE
    except ValueError:
        raise ListenerError("%s must be an octal mode such as 0660, not %r." % (OPTION_MODE, mode_text)) from None
    if mode not in (0o600, 0o660):
        raise ListenerError("%s must be 0600 or 0660, not %s." % (OPTION_MODE, oct(mode)))
    return {"path": path, "mode": mode, "group": (config.get(OPTION_GROUP) or "").strip(),
            "owner": (config.get(OPTION_OWNER) or "").strip()}


# [@ANCHOR: hams_rpc_guard:open_listener]
def open_listener(path, mode=DEFAULT_MODE, group="", owner="", backlog=BACKLOG):
    """Create, bind and listen on the Unix socket `path`, owned by `owner` (default: this process's user; another
    owner needs root) and group `group` (default: this process's group; another needs membership), with `mode`.
    Returns the non-blocking listening socket.

    A stale socket file (nobody accepts on it) is replaced; a live one (another server is listening), or a path that is
    not a socket, raises `ListenerError`, as do a missing directory, an unknown user or group and a failed chown. The
    socket is created under umask 0177, so it is never wider than 0600 before the final chmod."""
    if os.path.lexists(path):
        if not stat.S_ISSOCK(os.lstat(path).st_mode):
            raise ListenerError("%s exists and is not a socket; refusing to replace it." % path)
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            probe.connect(path)
        except OSError:
            os.unlink(path)  # stale: left by a server that stopped
        else:
            raise ListenerError("Another server is already listening on %s." % path)
        finally:
            probe.close()
    uids = {name: uid for uid, name in _user_names().items()}
    gids = {name: gid for gid, name in _group_names().items()}
    if (owner and owner not in uids) or (group and group not in gids):
        raise ListenerError("Unknown socket owner %r or group %r." % (owner, group))
    uid = uids[owner] if owner else -1
    gid = gids[group] if group else -1
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    old_umask = os.umask(0o177)
    try:
        sock.bind(path)
    except OSError as error:
        sock.close()
        raise ListenerError("Cannot bind %s: %s." % (path, error.strerror)) from None
    finally:
        os.umask(old_umask)
    try:
        if uid != -1 or gid != -1:
            os.chown(path, uid, gid)
        os.chmod(path, mode)
    except OSError as error:
        sock.close()
        os.unlink(path)
        raise ListenerError("Cannot set the owner, group or mode of %s: %s." % (path, error.strerror)) from None
    sock.listen(backlog)
    sock.setblocking(False)
    return sock


# [@ANCHOR: hams_rpc_guard:rpc_only_app]
def rpc_only_app(app):
    """A WSGI application that serves `app` (normally `odoo.http.root`) for the external RPC endpoints only, answering
    404 for every other path, and records the connection's verified `Peer` in the environ under
    `endpoint_gate.UNIX_PEER_ENVIRON_KEY` (as a dict). Forwarding headers are dropped."""
    def application(environ, start_response):
        if gate.endpoint_family(environ.get("PATH_INFO")) is None:
            body = b"Not Found: only the RPC endpoints are served on this socket.\n"
            start_response("404 NOT FOUND", [("Content-Type", "text/plain"), ("Content-Length", str(len(body)))])
            return [body]
        for key in _FORWARDING_ENVIRON_KEYS:
            environ.pop(key, None)
        return app(environ, start_response)
    return application


# [@ANCHOR: hams_rpc_guard:unix_request_handler]
class UnixRequestHandler(werkzeug.serving.WSGIRequestHandler):
    """werkzeug's request handler for one Unix-socket connection: one request per connection (HTTP/1.0), the peer's
    credentials read from the kernel and put in the environ, `REMOTE_ADDR` the loopback address (the caller *is* on
    this machine; Odoo code that parses `remote_addr` keeps working). The gate keys on the environ entry, never on
    `REMOTE_ADDR`."""

    protocol_version = "HTTP/1.0"
    timeout = SOCKET_TIMEOUT  # per socket operation, as Odoo's own worker: an idle client cannot hold a worker

    # [@ANCHOR: hams_rpc_guard:unix_request_handler_address_string]
    def address_string(self):
        return LOOPBACK_ADDRESS

    # [@ANCHOR: hams_rpc_guard:unix_request_handler_port_integer]
    def port_integer(self):
        return 0

    # [@ANCHOR: hams_rpc_guard:unix_request_handler_make_environ]
    def make_environ(self):
        environ = super().make_environ()
        environ[gate.UNIX_PEER_ENVIRON_KEY] = peer_of(self.connection).as_dict()
        return environ


# [@ANCHOR: hams_rpc_guard:unix_wsgi_server]
def make_wsgi_server(sock, app, threaded=False):
    """A werkzeug server object for the listening socket `sock` that serves `rpc_only_app(app)` with
    `UnixRequestHandler`. `threaded=True` gives a `ThreadedWSGIServer` to run with `serve_forever` in a thread;
    otherwise a `BaseWSGIServer` whose `process_request(conn, ("", 0))` a prefork worker calls per connection."""
    cls = werkzeug.serving.ThreadedWSGIServer if threaded else werkzeug.serving.BaseWSGIServer
    return cls("unix://" + sock.getsockname(), 0, rpc_only_app(app), handler=UnixRequestHandler, fd=sock.fileno())


# [@ANCHOR: hams_rpc_guard:serve_unix_connection]
def serve_unix_connection(server, connection):
    """Serve one accepted connection with `server` (from `make_wsgi_server`): blocking with Odoo's socket timeout,
    close-on-exec, a broken pipe tolerated as Odoo's own worker does."""
    connection.setblocking(True)
    fcntl.fcntl(connection, fcntl.F_SETFD, fcntl.fcntl(connection, fcntl.F_GETFD) | fcntl.FD_CLOEXEC)
    try:
        server.process_request(connection, ("", 0))
    except OSError as error:
        if error.errno != errno.EPIPE:
            raise


# [@ANCHOR: hams_rpc_guard:listener_socket]
def listener_socket():
    """The listening socket of this process, or None."""
    return _state["socket"]


# [@ANCHOR: hams_rpc_guard:ensure_listener]
def ensure_listener(config):
    """Open (or adopt from a reloading master, via `HAMS_RPC_GUARD_SOCKET_FD`) the configured listener once per
    process and return it, or None when none is configured or it cannot be created (logged at ERROR: the TCP server
    still runs, and with `rpc_transport = unix` the endpoints then answer nobody: closed, never open)."""
    with _state_lock:
        if _state["socket"] is not None:
            return _state["socket"]
        try:
            settings = listener_config(config)
        except ListenerError as error:
            _logger.error("RPC guard Unix socket not started: %s", error)
            return None
        if settings is None:
            return None
        inherited = os.environ.pop(INHERITED_FD_ENV, None)
        try:
            if inherited:
                sock = socket.socket(fileno=int(inherited))
                sock.setblocking(False)
            else:
                sock = open_listener(settings["path"], settings["mode"], settings["group"], settings["owner"])
        except (ListenerError, OSError, ValueError) as error:
            _logger.error("RPC guard Unix socket not started: %s", error)
            return None
        _state.update(socket=sock, path=settings["path"])
        _logger.info("RPC guard: serving the RPC endpoints on the Unix socket %s as well (mode %s).",
                     settings["path"], oct(settings["mode"]))
        return sock


# [@ANCHOR: hams_rpc_guard:install_server_patches]
def install_server_patches():
    """Teach Odoo's prefork and threaded servers about the listener (idempotent; called from `post_load`, so in the
    prefork master before any worker exists). Does nothing at all unless the configuration names a socket path, and
    then only when a server actually starts serving HTTP."""
    if _state["patched"]:
        return
    running = odoo_server.server
    if (isinstance(running, odoo_server.PreforkServer) and running.pid != os.getpid()
            and (config.get(OPTION_PATH) or "").strip()):
        _logger.error("RPC guard Unix socket not started: hams_rpc_guard was first loaded in a worker process. Add it "
                      "to server_wide_modules (or db_name, so the registry is preloaded in the master).")

    prefork_start = odoo_server.PreforkServer.start
    prefork_reload = odoo_server.PreforkServer.fork_and_reload
    prefork_spawn = odoo_server.PreforkServer.process_spawn
    worker_start = odoo_server.WorkerHTTP.start
    worker_sleep = odoo_server.WorkerHTTP.sleep
    worker_work = odoo_server.WorkerHTTP.process_work
    cron_start = odoo_server.WorkerCron.start
    http_spawn = odoo_server.ThreadedServer.http_spawn

    def start(self, *args, **kwargs):
        result = prefork_start(self, *args, **kwargs)
        if config["http_enable"]:
            ensure_listener(config)
        return result

    def process_spawn(self):
        # The master, before it forks workers: covers a module loaded while the registry was preloaded (db_name),
        # after `start` had run.
        if config["http_enable"]:
            ensure_listener(config)
        return prefork_spawn(self)

    def fork_and_reload(self):
        sock = listener_socket()
        if sock is not None:
            fd = sock.fileno()
            fcntl.fcntl(fd, fcntl.F_SETFD, fcntl.fcntl(fd, fcntl.F_GETFD) & ~fcntl.FD_CLOEXEC)
            os.environ[INHERITED_FD_ENV] = str(fd)
        return prefork_reload(self)

    def http_worker_start(self):
        worker_start(self)
        sock = listener_socket()
        _state["server"] = make_wsgi_server(sock, self.multi.app) if sock is not None else None

    def http_worker_sleep(self):
        sock = listener_socket()
        if sock is None:
            return worker_sleep(self)
        try:
            select.select([self.multi.socket, sock, self.wakeup_fd_r], [], [], self.multi.beat)
            odoo_server.empty_pipe(self.wakeup_fd_r)
        except OSError as error:
            if error.args[0] != errno.EINTR:
                raise

    def http_worker_process_work(self):
        sock = listener_socket()
        if sock is None or _state["server"] is None:
            return worker_work(self)
        try:
            connection, _addr = sock.accept()
        except OSError as error:
            if error.errno not in (errno.EAGAIN, errno.ECONNABORTED):
                raise
            return worker_work(self)  # nothing on the Unix socket: Odoo's own TCP accept, unchanged
        serve_unix_connection(_state["server"], connection)
        self.request_count += 1

    def cron_worker_start(self):
        sock = listener_socket()
        if sock is not None:
            sock.close()
            _state["socket"] = None
        return cron_start(self)

    def threaded_http_spawn(self):
        http_spawn(self)
        sock = ensure_listener(config)
        if sock is not None:
            server = make_wsgi_server(sock, self.app, threaded=True)
            thread = threading.Thread(  # burn-ignore-bounded-daemon-thread: one per process, started once by http_spawn (ensure_listener opens the socket once), ends with the process like Odoo's own httpd thread
                target=server.serve_forever, name="hams_rpc_guard.unix_httpd", daemon=True)
            thread.start()

    odoo_server.PreforkServer.start = start
    odoo_server.PreforkServer.fork_and_reload = fork_and_reload
    odoo_server.PreforkServer.process_spawn = process_spawn
    odoo_server.WorkerHTTP.start = http_worker_start
    odoo_server.WorkerHTTP.sleep = http_worker_sleep
    odoo_server.WorkerHTTP.process_work = http_worker_process_work
    odoo_server.WorkerCron.start = cron_worker_start
    odoo_server.ThreadedServer.http_spawn = threaded_http_spawn
    _state["patched"] = True
