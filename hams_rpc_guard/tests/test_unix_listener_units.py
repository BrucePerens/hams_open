# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests of unix_listener.py with real sockets in a scratch directory and fake Odoo server classes, added to kill
the mutants the end-to-end tests left alive (mutation run 2026-11-13). No Odoo HTTP server is involved."""
import errno
import fcntl
import gc
import ipaddress
import os
import shutil
import socket
import struct
import tempfile
import warnings
from types import SimpleNamespace

import werkzeug.serving

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_rpc_guard import endpoint_gate as gate, unix_listener

OPT = unix_listener


def _scratch(case):
    path = tempfile.mkdtemp(prefix="hrg-")  # short: a Unix socket path is limited to about 107 bytes
    case.addCleanup(shutil.rmtree, path, True)
    return path


def _fresh_state(**values):
    state = {"socket": None, "path": None, "server": None, "patched": False}
    state.update(values)
    return state


@tagged("post_install", "-at_install")
class TestListenerConfigAndSocket(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:listener_config]
    # Tests [@ANCHOR: hams_rpc_guard:open_listener]
    # Tests [@ANCHOR: hams_rpc_guard:peer_of]
    # Tests [@ANCHOR: hams_rpc_guard:user_names]
    # Tests [@ANCHOR: hams_rpc_guard:group_names]

    def test_listener_config_values_and_messages(self):
        cfg = unix_listener.listener_config
        self.assertIsNone(cfg({OPT.OPTION_PATH: None}))
        self.assertIsNone(cfg({OPT.OPTION_PATH: "   "}))
        self.assertEqual(cfg({OPT.OPTION_PATH: " /r/s ", OPT.OPTION_OWNER: " odoo ", OPT.OPTION_GROUP: " g "}),
                         {"path": "/r/s", "mode": 0o660, "group": "g", "owner": "odoo"})
        self.assertEqual(cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: " 0600 "})["mode"], 0o600)
        self.assertEqual(cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: "0660"})["mode"], 0o660)
        self.assertEqual(cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: ""})["mode"], 0o660)
        self.assertEqual(cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: None})["mode"], 0o660)
        with self.assertRaises(unix_listener.ListenerError) as caught:
            cfg({OPT.OPTION_PATH: "rpc.sock"})
        self.assertEqual(str(caught.exception), "hams_rpc_guard_socket must be an absolute path, not 'rpc.sock'.")
        with self.assertRaises(unix_listener.ListenerError) as caught:
            cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: "rw"})
        self.assertEqual(str(caught.exception), "hams_rpc_guard_socket_mode must be an octal mode such as 0660, not 'rw'.")
        for mode in ("0", "0640", "0666", "0777", "0700", "0606", "1660", "8", "0o600x"):
            with self.assertRaises(unix_listener.ListenerError, msg=mode):
                cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: mode})
        with self.assertRaises(unix_listener.ListenerError) as caught:
            cfg({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: "0640"})
        self.assertEqual(str(caught.exception), "hams_rpc_guard_socket_mode must be 0600 or 0660, not 0o640.")

    def test_open_listener_socket_properties(self):
        path = os.path.join(_scratch(self), "rpc.sock")
        before = os.umask(0o022)
        try:
            sock = unix_listener.open_listener(path, 0o600)
            after = os.umask(0o022)
        finally:
            os.umask(before)
        self.addCleanup(sock.close)
        self.assertEqual(after, 0o022, "the umask is restored")
        self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
        self.assertEqual(sock.gettimeout(), 0.0, "non-blocking")
        self.assertEqual(sock.getsockname(), path)
        self.assertEqual(sock.family, socket.AF_UNIX)
        self.assertEqual(sock.type, socket.SOCK_STREAM)
        sock.close()
        again = unix_listener.open_listener(path, 0o660, backlog=1)  # a stale file is replaced
        self.addCleanup(again.close)
        self.assertEqual(os.stat(path).st_mode & 0o777, 0o660)

    def test_open_listener_refusals_and_their_messages(self):
        directory = _scratch(self)
        path = os.path.join(directory, "rpc.sock")
        live = unix_listener.open_listener(path)
        self.addCleanup(live.close)
        with self.assertRaises(unix_listener.ListenerError) as caught:
            unix_listener.open_listener(path)
        self.assertEqual(str(caught.exception), "Another server is already listening on %s." % path)
        self.assertTrue(os.path.exists(path), "a live socket is left alone")
        regular = os.path.join(directory, "file")
        with open(regular, "w") as handle:
            handle.write("x")
        with self.assertRaises(unix_listener.ListenerError) as caught:
            unix_listener.open_listener(regular)
        self.assertEqual(str(caught.exception), "%s exists and is not a socket; refusing to replace it." % regular)
        self.assertEqual(open(regular).read(), "x")
        link = os.path.join(directory, "link")
        os.symlink(regular, link)
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(link)
        with self.assertRaises(unix_listener.ListenerError) as caught:
            unix_listener.open_listener(os.path.join(directory, "x.sock"), owner="no-such-user-hrg")
        self.assertEqual(str(caught.exception), "Unknown socket owner 'no-such-user-hrg' or group ''.")
        with self.assertRaises(unix_listener.ListenerError) as caught:
            unix_listener.open_listener(os.path.join(directory, "x.sock"), group="no-such-group-hrg")
        self.assertEqual(str(caught.exception), "Unknown socket owner '' or group 'no-such-group-hrg'.")
        self.assertFalse(os.path.exists(os.path.join(directory, "x.sock")), "nothing was created")
        missing = os.path.join(directory, "missing", "rpc.sock")
        with self.assertRaises(unix_listener.ListenerError) as caught:
            unix_listener.open_listener(missing)
        self.assertEqual(str(caught.exception), "Cannot bind %s: No such file or directory." % missing)

    def _open_fds(self):
        return len(os.listdir("/proc/self/fd"))

    def test_open_listener_closes_what_it_opens_on_every_path(self):
        with warnings.catch_warnings(record=True) as caught_warnings:
            warnings.simplefilter("always")
            self._closes_what_it_opens()
            gc.collect()
        leaks = [str(w.message) for w in caught_warnings if issubclass(w.category, ResourceWarning)]
        self.assertEqual(leaks, [], "no socket is left for the garbage collector to close")

    def _closes_what_it_opens(self):
        directory = _scratch(self)
        path = os.path.join(directory, "rpc.sock")
        stale = unix_listener.open_listener(path)
        stale.close()  # leaves a stale socket file that the next call probes with a connect
        baseline = self._open_fds()
        kept = unix_listener.open_listener(path)
        self.addCleanup(kept.close)
        self.assertEqual(self._open_fds(), baseline + 1, "the probe socket is closed; only the listener remains")
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(path)  # a live one: the probe is closed again
        self.assertEqual(self._open_fds(), baseline + 1)
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(os.path.join(directory, "missing", "x.sock"))  # bind fails
        self.assertEqual(self._open_fds(), baseline + 1, "the socket of a failed bind is closed")

        def refuse(*args):
            raise PermissionError(errno.EPERM, "Operation not permitted")
        self.safe_patch_object(unix_listener.os, "chmod", new=refuse)
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(os.path.join(directory, "y.sock"))  # chmod fails
        self.assertEqual(self._open_fds(), baseline + 1, "the socket of a failed chmod is closed")

    def test_the_socket_is_never_wider_than_0600_before_the_final_chmod(self):
        path = os.path.join(_scratch(self), "rpc.sock")
        seen = []
        real_chmod = os.chmod

        def watch(target, mode, *args, **kwargs):
            seen.append(os.stat(target).st_mode & 0o777)
            return real_chmod(target, mode, *args, **kwargs)
        self.safe_patch_object(unix_listener.os, "chmod", new=watch)
        before = os.umask(0o000)  # a permissive caller umask must not matter
        try:
            sock = unix_listener.open_listener(path, 0o660)
        finally:
            os.umask(before)
        self.addCleanup(sock.close)
        self.assertEqual(seen, [0o600])
        self.assertEqual(os.stat(path).st_mode & 0o777, 0o660)

    def test_open_listener_failed_chown_removes_the_socket_and_names_the_cause(self):
        directory = _scratch(self)
        path = os.path.join(directory, "rpc.sock")

        def refuse(*args):
            raise PermissionError(errno.EPERM, "Operation not permitted")
        self.safe_patch_object(unix_listener.os, "chown", new=refuse)
        group = unix_listener._group_names()[os.getgid()]
        with self.assertRaises(unix_listener.ListenerError) as caught:
            unix_listener.open_listener(path, group=group)
        self.assertEqual(str(caught.exception),
                         "Cannot set the owner, group or mode of %s: Operation not permitted." % path)
        self.assertFalse(os.path.lexists(path), "a half-made socket is not left behind")

    def test_open_listener_chmod_failure_removes_the_socket(self):
        directory = _scratch(self)
        path = os.path.join(directory, "rpc.sock")

        def refuse(*args):
            raise PermissionError(errno.EPERM, "Operation not permitted")
        self.safe_patch_object(unix_listener.os, "chmod", new=refuse)
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(path)
        self.assertFalse(os.path.lexists(path))

    def test_open_listener_only_chowns_when_asked(self):
        calls = []
        real_chown = os.chown
        self.safe_patch_object(unix_listener.os, "chown", new=lambda *a: calls.append(a) or real_chown(*a))
        directory = _scratch(self)
        first = unix_listener.open_listener(os.path.join(directory, "a.sock"))
        self.addCleanup(first.close)
        self.assertEqual(calls, [], "no owner and no group: no chown")
        user = unix_listener._user_names()[os.getuid()]
        second = unix_listener.open_listener(os.path.join(directory, "b.sock"), owner=user)
        self.addCleanup(second.close)
        self.assertEqual(calls, [(os.path.join(directory, "b.sock"), os.getuid(), -1)])
        group = unix_listener._group_names()[os.getgid()]
        third = unix_listener.open_listener(os.path.join(directory, "c.sock"), group=group)
        self.addCleanup(third.close)
        self.assertEqual(calls[-1], (os.path.join(directory, "c.sock"), -1, os.getgid()))

    def test_peer_of_reads_the_credentials_and_names(self):
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        peer = unix_listener.peer_of(left)
        self.assertEqual((peer.pid, peer.uid, peer.gid), (os.getpid(), os.getuid(), os.getgid()))
        self.assertEqual(peer.user, unix_listener._user_names()[os.getuid()])
        self.assertEqual(peer.group, unix_listener._group_names()[os.getgid()])

        class Fake:
            def getsockopt(self, level, option, size):
                assert (level, option, size) == (socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
                return struct.pack("3i", 7, 4242424, 4242425)
        stranger = unix_listener.peer_of(Fake())
        self.assertEqual((stranger.pid, stranger.uid, stranger.gid, stranger.user, stranger.group),
                         (7, 4242424, 4242425, "", ""))

    def test_name_tables(self):
        users = unix_listener._user_names()
        self.assertEqual(users[0], "root")
        self.assertEqual(unix_listener._group_names()[0], "root")


@tagged("post_install", "-at_install")
class TestRpcOnlyApp(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:rpc_only_app]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler_address_string]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler_port_integer]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler_make_environ]
    # Tests [@ANCHOR: hams_rpc_guard:unix_wsgi_server]
    # Tests [@ANCHOR: hams_rpc_guard:serve_unix_connection]

    def _run(self, environ):
        seen = {}

        def app(env, start_response):
            seen["environ"] = env
            start_response("200 OK", [])
            return [b"served"]

        started = []
        body = unix_listener.rpc_only_app(app)(environ, lambda status, headers: started.append((status, headers)))
        return seen, started, body

    def test_other_paths_get_a_404_and_never_reach_the_app(self):
        for path in ("/web", "/", "/xmlrpc.php", "/web/login", "", None):
            environ = {} if path is None else {"PATH_INFO": path}
            seen, started, body = self._run(environ)
            self.assertEqual(seen, {}, path)
            body_text = b"Not Found: only the RPC endpoints are served on this socket.\n"
            self.assertEqual(body, [body_text], path)
            self.assertEqual(started, [("404 NOT FOUND", [("Content-Type", "text/plain"),
                                                          ("Content-Length", str(len(body_text)))])], path)

    def test_rpc_paths_reach_the_app_without_forwarding_headers(self):
        for path in ("/jsonrpc", "/xmlrpc/2/common", "/json/2/res.users/search"):
            environ = {"PATH_INFO": path, "HTTP_X_FORWARDED_FOR": "1.2.3.4", "HTTP_VIA": "x", "HTTP_CF_RAY": "r",
                       "HTTP_TRUE_CLIENT_IP": "1.1.1.1", "HTTP_FORWARDED": "for=1", "HTTP_X_REAL_IP": "2.2.2.2",
                       "HTTP_AUTHORIZATION": "keep", "HTTP_X_ODOO_DATABASE": "keep"}
            seen, started, body = self._run(environ)
            self.assertEqual(body, [b"served"], path)
            self.assertEqual(started, [("200 OK", [])])
            self.assertIs(seen["environ"], environ)
            self.assertEqual(sorted(seen["environ"]), ["HTTP_AUTHORIZATION", "HTTP_X_ODOO_DATABASE", "PATH_INFO"], path)

    def test_every_forwarding_header_has_its_environ_key_dropped(self):
        self.assertEqual(set(unix_listener._FORWARDING_ENVIRON_KEYS),
                         {"HTTP_X_FORWARDED_FOR", "HTTP_X_FORWARDED_HOST", "HTTP_X_FORWARDED_PROTO",
                          "HTTP_X_FORWARDED_SERVER", "HTTP_X_REAL_IP", "HTTP_FORWARDED", "HTTP_VIA",
                          "HTTP_CF_CONNECTING_IP", "HTTP_CF_RAY", "HTTP_TRUE_CLIENT_IP"})

    def test_the_request_handler_pins_loopback_and_adds_the_verified_peer(self):
        handler = unix_listener.UnixRequestHandler.__new__(unix_listener.UnixRequestHandler)
        self.assertEqual(ipaddress.ip_address(handler.address_string()), ipaddress.IPv4Address(0x7F000001))
        self.assertEqual(handler.port_integer(), 0)
        self.assertEqual(unix_listener.UnixRequestHandler.protocol_version, "HTTP/1.0")
        self.assertEqual(unix_listener.UnixRequestHandler.timeout, 2.0)
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        handler.connection = left
        self.safe_patch_object(werkzeug.serving.WSGIRequestHandler, "make_environ", new=lambda self: {"A": 1})
        environ = handler.make_environ()
        self.assertEqual(environ["A"], 1)
        self.assertEqual(environ[gate.UNIX_PEER_ENVIRON_KEY],
                         {"pid": os.getpid(), "uid": os.getuid(), "gid": os.getgid(),
                          "user": unix_listener._user_names()[os.getuid()],
                          "group": unix_listener._group_names()[os.getgid()]})

    def test_make_wsgi_server_picks_the_class_and_wraps_the_app(self):
        path = os.path.join(_scratch(self), "w.sock")
        sock = unix_listener.open_listener(path)
        self.addCleanup(sock.close)
        for threaded, cls in ((False, werkzeug.serving.BaseWSGIServer), (True, werkzeug.serving.ThreadedWSGIServer)):
            server = unix_listener.make_wsgi_server(sock, lambda e, s: [b""], threaded=threaded)
            self.addCleanup(server.server_close)
            self.assertIs(type(server), cls)
            self.assertIs(server.RequestHandlerClass, unix_listener.UnixRequestHandler)
            self.assertEqual(server.socket.getsockname(), path)
            self.assertEqual((server.host, server.port), ("unix://" + path, 0))
        default = unix_listener.make_wsgi_server(sock, lambda e, s: [b""])
        self.addCleanup(default.server_close)
        self.assertIs(type(default), werkzeug.serving.BaseWSGIServer)
        started = []
        default.app({"PATH_INFO": "/nope"}, lambda status, headers: started.append(status))
        self.assertEqual(started, ["404 NOT FOUND"], "the server's app is the RPC-only wrapper")

    def test_serve_unix_connection(self):
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        left.setblocking(False)
        self.assertTrue(fcntl.fcntl(left, fcntl.F_GETFD) & fcntl.FD_CLOEXEC, "Python sockets start close-on-exec")
        fcntl.fcntl(left, fcntl.F_SETFD, 0)
        calls = []
        server = SimpleNamespace(process_request=lambda conn, addr: calls.append((conn, addr)))
        unix_listener.serve_unix_connection(server, left)
        self.assertEqual(calls, [(left, ("", 0))])
        self.assertIsNone(left.gettimeout(), "back to blocking")
        self.assertTrue(fcntl.fcntl(left, fcntl.F_GETFD) & fcntl.FD_CLOEXEC)

        def epipe(conn, addr):
            raise OSError(errno.EPIPE, "broken")
        unix_listener.serve_unix_connection(SimpleNamespace(process_request=epipe), left)  # tolerated

        def other(conn, addr):
            raise OSError(errno.ECONNRESET, "reset")
        with self.assertRaises(OSError) as caught:
            unix_listener.serve_unix_connection(SimpleNamespace(process_request=other), left)
        self.assertEqual(caught.exception.errno, errno.ECONNRESET)


@tagged("post_install", "-at_install")
class TestEnsureListener(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:ensure_listener]
    # Tests [@ANCHOR: hams_rpc_guard:listener_socket]

    def setUp(self):
        super().setUp()
        self.state = _fresh_state()
        self.safe_patch_object(unix_listener, "_state", new=self.state)
        self.safe_patch_object(unix_listener.os.environ, "pop", new=self._pop)
        self.env_fd = None

    def _pop(self, key, default=None):
        return self.env_fd if key == unix_listener.INHERITED_FD_ENV else default

    def test_a_listener_that_exists_is_returned_untouched(self):
        marker = object()
        self.state["socket"] = marker
        self.assertIs(unix_listener.ensure_listener({OPT.OPTION_PATH: "rel"}), marker)
        self.assertIs(unix_listener.listener_socket(), marker)

    def test_it_opens_once_records_the_path_and_logs_the_mode(self):
        path = os.path.join(_scratch(self), "e.sock")
        with self.assertLogs(unix_listener._logger, "INFO") as logs:
            sock = unix_listener.ensure_listener({OPT.OPTION_PATH: path, OPT.OPTION_MODE: "0600"})
        self.addCleanup(sock.close)
        self.assertIs(self.state["socket"], sock)
        self.assertEqual(self.state["path"], path)
        self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
        self.assertEqual(logs.records[0].getMessage(),
                         "RPC guard: serving the RPC endpoints on the Unix socket %s as well (mode 0o600)." % path)
        self.assertIs(unix_listener.ensure_listener({OPT.OPTION_PATH: path}), sock)
        self.assertIs(unix_listener.listener_socket(), sock)

    def test_a_bad_setting_or_an_unopenable_path_is_logged_and_opens_nothing(self):
        with self.assertLogs(unix_listener._logger, "ERROR") as logs:
            self.assertIsNone(unix_listener.ensure_listener({OPT.OPTION_PATH: "rel"}))
        self.assertEqual(logs.records[0].getMessage(), "RPC guard Unix socket not started: "
                                                       "hams_rpc_guard_socket must be an absolute path, not 'rel'.")
        missing = os.path.join(_scratch(self), "nope", "e.sock")
        with self.assertLogs(unix_listener._logger, "ERROR") as logs:
            self.assertIsNone(unix_listener.ensure_listener({OPT.OPTION_PATH: missing}))
        self.assertEqual(logs.records[0].getMessage(), "RPC guard Unix socket not started: Cannot bind %s: "
                                                       "No such file or directory." % missing)
        self.assertIsNone(self.state["socket"])
        self.assertIsNone(self.state["path"])

    def test_an_inherited_descriptor_is_adopted_not_reopened(self):
        path = os.path.join(_scratch(self), "i.sock")
        original = unix_listener.open_listener(path)
        self.addCleanup(original.close)
        self.env_fd = str(os.dup(original.fileno()))
        calls = []
        self.safe_patch_object(unix_listener, "open_listener", new=lambda *a: calls.append(a))
        sock = unix_listener.ensure_listener({OPT.OPTION_PATH: path})
        self.addCleanup(sock.close)
        self.assertEqual(calls, [], "no new socket was bound")
        self.assertEqual(sock.getsockname(), path)
        self.assertEqual(sock.gettimeout(), 0.0, "non-blocking")
        self.assertEqual(self.state["path"], path)

    def test_a_junk_inherited_descriptor_is_logged(self):
        self.env_fd = "not-a-number"
        with self.assertLogs(unix_listener._logger, "ERROR") as logs:
            self.assertIsNone(unix_listener.ensure_listener({OPT.OPTION_PATH: os.path.join(_scratch(self), "never.sock")}))
        self.assertTrue(logs.records[0].getMessage().startswith("RPC guard Unix socket not started: "))
        self.assertIsNone(self.state["socket"])

    def test_open_listener_receives_the_configured_settings(self):
        calls = []
        sentinel = socket.socket(socket.AF_UNIX)
        self.addCleanup(sentinel.close)
        self.safe_patch_object(unix_listener, "open_listener", new=lambda *a: calls.append(a) or sentinel)
        unix_listener.ensure_listener({OPT.OPTION_PATH: "/r/s", OPT.OPTION_MODE: "0600", OPT.OPTION_GROUP: "g",
                                       OPT.OPTION_OWNER: "o"})
        self.assertEqual(calls, [("/r/s", 0o600, "g", "o")])
