# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import errno
import grp
import os
import pwd
import shutil
import socket
import stat
import sys
import tempfile
import threading
import urllib.parse
from types import SimpleNamespace

from werkzeug.datastructures import Headers

import odoo.http
from odoo.addons.zero_sudo.models.security_utils import ZeroSudoSecurityUtils
from odoo.exceptions import AccessError
from odoo.service import server as odoo_server
from odoo.addons.zero_sudo.daemon.odoo_rpc_client import OdooRpcClient, OdooRpcError, UnixHTTPConnection
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_rpc_guard import endpoint_gate as gate, patch as guard, policy, unix_listener

VERSION_CALL = b'<?xml version="1.0"?><methodCall><methodName>version</methodName><params/></methodCall>'


def _scratch_dir(case):
    # Short: a Unix socket path is limited to about 107 bytes.
    path = tempfile.mkdtemp(prefix="hrg-")
    case.addCleanup(shutil.rmtree, path, True)
    return path


@tagged("post_install", "-at_install")
class TestPeerRules(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:parse_peer_rules]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rules]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rule]
    # Tests [@ANCHOR: hams_rpc_guard:peer]
    # Tests [@ANCHOR: hams_rpc_guard:peer_init]
    # Tests [@ANCHOR: hams_rpc_guard:peer_as_dict]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rule_init]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rule_matches]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rules_init]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rules_match]
    # Tests [@ANCHOR: hams_rpc_guard:judge_unix]

    def test_entries(self):
        rules = gate.parse_peer_rules(" hamsd_fcc=callbook_sync|other , 1234, @hamsd , @998=svc ")
        self.assertEqual([(r.kind, r.value, sorted(r.logins)) for r in rules.rules],
                         [("user", "hamsd_fcc", ["callbook_sync", "other"]), ("uid", 1234, []),
                          ("group", "hamsd", []), ("gid", 998, ["svc"])])
        self.assertEqual(gate.parse_peer_rules(None).rules, ())

    def test_malformed_entries_are_errors(self):
        for value in ("=login", "user=", "user=a|", "bad name", "@", "us/er", "user=a b",
                      "\u00b2", "@\u00b2=svc"):  # superscript two: str.isdigit() is true, int() raises ValueError
            with self.subTest(value=value), self.assertRaises(gate.ConfigError):
                gate.parse_peer_rules(value)

    def test_matching_takes_the_first_rule(self):
        rules = gate.parse_peer_rules("alice=a, @staff=s, 2000")
        alice = gate.Peer(10, 1001, 50, "alice", "staff")
        bob = gate.Peer(11, 1002, 50, "bob", "staff")
        carol = gate.Peer(12, 2000, 60, "carol", "users")
        eve = gate.Peer(13, 3000, 60, "eve", "users")
        self.assertEqual(rules.match(alice).logins, {"a"})
        self.assertEqual(rules.match(bob).logins, {"s"})
        self.assertEqual(rules.match(carol).logins, frozenset())
        self.assertIsNone(rules.match(eve))

    def test_judge_unix(self):
        settings = gate.GateSettings((), transport=gate.TRANSPORT_UNIX,
                                     unix_peers=gate.parse_peer_rules("alice=a"))
        alice = gate.Peer(10, 1001, 50, "alice", "staff")
        eve = gate.Peer(13, 3000, 60, "eve", "users")
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, settings, alice), (True, "allowed", {"a"}))
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, settings, eve)[:2], (False, "peer"))
        settings.transport = gate.TRANSPORT_TCP
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, settings, alice)[:2], (False, "transport"))
        settings.transport, settings.legacy = gate.TRANSPORT_BOTH, gate.LEGACY_NONE
        self.assertEqual(gate.judge_unix(gate.FAMILY_LEGACY, settings, alice)[:2], (False, "legacy-closed"))
        settings.unix_peers = None  # the setting could not be read
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, settings, alice)[:2], (False, "peer"))


@tagged("post_install", "-at_install")
class TestListenerSocket(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:listener_config]
    # Tests [@ANCHOR: hams_rpc_guard:user_names]
    # Tests [@ANCHOR: hams_rpc_guard:group_names]
    # Tests [@ANCHOR: hams_rpc_guard:listener_options]
    # Tests [@ANCHOR: hams_rpc_guard:open_listener]
    # Tests [@ANCHOR: hams_rpc_guard:peer_of]

    def test_config(self):
        self.assertIsNone(unix_listener.listener_config({}))
        self.assertEqual(unix_listener.listener_config({unix_listener.OPTION_PATH: "/run/odoo/rpc.sock"}),
                         {"path": "/run/odoo/rpc.sock", "mode": 0o660, "group": "", "owner": ""})
        self.assertEqual(unix_listener.listener_config({unix_listener.OPTION_PATH: "/r/s", unix_listener.OPTION_MODE: "600",
                                                        unix_listener.OPTION_GROUP: " hamsd "})["mode"], 0o600)
        for bad in ({unix_listener.OPTION_PATH: "rpc.sock"},
                    {unix_listener.OPTION_PATH: "/r/s", unix_listener.OPTION_MODE: "0666"},
                    {unix_listener.OPTION_PATH: "/r/s", unix_listener.OPTION_MODE: "0777"},
                    {unix_listener.OPTION_PATH: "/r/s", unix_listener.OPTION_MODE: "rw"}):
            with self.subTest(bad=bad), self.assertRaises(unix_listener.ListenerError):
                unix_listener.listener_config(bad)

    def test_socket_permissions_and_peer_credentials(self):
        path = os.path.join(_scratch_dir(self), "rpc.sock")
        group = grp.getgrgid(os.getgid()).gr_name
        sock = unix_listener.open_listener(path, 0o660, group=group)
        self.addCleanup(sock.close)
        info = os.stat(path)
        self.assertTrue(stat.S_ISSOCK(info.st_mode))
        self.assertEqual(stat.S_IMODE(info.st_mode), 0o660)
        self.assertEqual((info.st_uid, info.st_gid), (os.getuid(), os.getgid()))
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(client.close)
        client.connect(path)
        sock.setblocking(True)
        connection, _addr = sock.accept()
        self.addCleanup(connection.close)
        peer = unix_listener.peer_of(connection)
        self.assertEqual((peer.pid, peer.uid, peer.gid), (os.getpid(), os.getuid(), os.getgid()))
        self.assertEqual(peer.user, pwd.getpwuid(os.getuid()).pw_name)

    def test_a_live_socket_or_a_regular_file_is_never_replaced_but_a_stale_one_is(self):
        directory = _scratch_dir(self)
        path = os.path.join(directory, "rpc.sock")
        live = unix_listener.open_listener(path, 0o600)
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(path, 0o600)
        live.close()  # the file stays: stale
        again = unix_listener.open_listener(path, 0o600)
        again.close()
        regular = os.path.join(directory, "file")
        with open(regular, "w") as handle:
            handle.write("x")
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(regular, 0o600)
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(os.path.join(directory, "missing", "rpc.sock"))
        with self.assertRaises(unix_listener.ListenerError):
            unix_listener.open_listener(os.path.join(directory, "x.sock"), group="no-such-group-hrg")

    def test_server_hooks_are_installed_once(self):
        # Tests [@ANCHOR: hams_rpc_guard:install_server_patches]
        # Tests [@ANCHOR: hams_rpc_guard:listener_socket]
        unix_listener.install_server_patches()
        sleep = odoo_server.WorkerHTTP.sleep
        unix_listener.install_server_patches()
        self.assertIs(odoo_server.WorkerHTTP.sleep, sleep)
        self.assertTrue(unix_listener._state["patched"])
        self.assertIsNone(unix_listener.listener_socket(), "no socket unless the configuration names one")

    def test_the_tcp_fallback_of_a_worker_runs_outside_the_except_block(self):
        """With the Unix socket idle (EAGAIN) the worker falls back to Odoo's own TCP accept and request. That must not run inside
        the `except` block of the Unix accept: a traceback Odoo then logs was chained to a spurious BlockingIOError
        ("During handling of the above exception..."), found in the log of the 2026-11-12 rehearsal."""
        # Tests [@ANCHOR: hams_rpc_guard:install_server_patches]
        unix_listener.install_server_patches()
        seen = []

        class IdleUnixSocket:
            def accept(self):
                raise BlockingIOError(errno.EAGAIN, "idle")

        class TcpSocket:
            def accept(self):
                seen.append(sys.exception())  # None unless called from inside an except block
                raise BlockingIOError(errno.EAGAIN, "idle")

        worker = SimpleNamespace(multi=SimpleNamespace(socket=TcpSocket()), request_count=0)
        self.safe_patch_object(unix_listener, "listener_socket", new=lambda: IdleUnixSocket())
        self.safe_patch_object(unix_listener, "_state", new={"server": object(), "socket": None, "patched": True})
        odoo_server.WorkerHTTP.process_work(worker)
        self.assertEqual(seen, [None])

    def test_ensure_listener_without_a_path_or_with_a_bad_one_opens_nothing(self):
        # Tests [@ANCHOR: hams_rpc_guard:ensure_listener]
        self.assertIsNone(unix_listener.ensure_listener({}))
        with self.assertLogs(unix_listener._logger, "ERROR"):
            self.assertIsNone(unix_listener.ensure_listener({unix_listener.OPTION_PATH: "relative.sock"}))


@tagged("post_install", "-at_install")
class TestUnixSocketEndToEnd(HamsHttpCase):
    """The listener serving `odoo.http.root` on a scratch socket in this test's process (the threaded server path),
    called with the daemon client helper. A bearer key that is not one gets Odoo's own 401, which proves the request
    went through the gate to the core's authentication; the gate's refusal is a 404."""
    # Tests [@ANCHOR: hams_rpc_guard:unix_wsgi_server]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler_address_string]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler_port_integer]
    # Tests [@ANCHOR: hams_rpc_guard:unix_request_handler_make_environ]
    # Tests [@ANCHOR: hams_rpc_guard:rpc_only_app]
    # Tests [@ANCHOR: hams_rpc_guard:serve_unix_connection]
    # Tests [@ANCHOR: zero_sudo:odoo_rpc_client]
    # Tests [@ANCHOR: zero_sudo:odoo_rpc_client_call]
    # Tests [@ANCHOR: zero_sudo:unix_http_connection]
    # Tests [@ANCHOR: zero_sudo:unix_http_connection_init]
    # Tests [@ANCHOR: zero_sudo:unix_http_connection_connect]
    # Tests [@ANCHOR: zero_sudo:odoo_rpc_error_init]
    # Tests [@ANCHOR: zero_sudo:odoo_rpc_client_init]
    # Tests [@ANCHOR: zero_sudo:odoo_rpc_client_connection]

    def setUp(self):
        super().setUp()
        self.values = {}

        def read(utils, key, default=None):
            value = self.values.get(key)
            return value if value is not None else default
        self.safe_patch_object(ZeroSudoSecurityUtils, "_get_system_param", new=read)
        self.path = os.path.join(_scratch_dir(self), "rpc.sock")
        listener = unix_listener.open_listener(self.path, 0o600)
        self.addCleanup(listener.close)
        server = unix_listener.make_wsgi_server(listener, odoo.http.root, threaded=True)
        thread = threading.Thread(  # burn-ignore-test-daemon-thread: one per test, stopped by server.shutdown in cleanup
            target=server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True)
        thread.start()
        self.addCleanup(thread.join, 5)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        self.me = pwd.getpwuid(os.getuid()).pw_name
        # The test server's dbfilter may select the database by host name: send the test server's own.
        self.host = urllib.parse.urlsplit(self.base_url()).netloc
        self.unix = OdooRpcClient("unix://" + self.path, "not-a-real-api-key", self.env.cr.dbname, host=self.host)

    def _requests(self):
        """HttpCase lets a request use the test cursor only inside allow_requests (it releases the test lock); the
        daemon client sends no test cookie, hence all_requests. Not held for the whole test: HamsHttpCase.tearDown
        does its own url_open and would take the lock a second time."""
        return self.allow_requests(all_requests=True)

    def _status(self, client=None):
        with self._requests(), self.assertRaises(OdooRpcError) as caught:
            (client or self.unix).call("res.users", "context_get")
        return caught.exception.status

    def _version(self, path="/xmlrpc/2/common"):
        connection = UnixHTTPConnection(self.path, timeout=10, host=self.host)
        try:
            with self._requests():
                connection.request("POST", path, body=VERSION_CALL, headers={"Content-Type": "text/xml"})
                response = connection.getresponse()
                return response.status, response.read()
        finally:
            connection.close()

    def test_served_on_the_socket_only_when_the_operator_says_so(self):
        self.values = {gate.UNIX_PEERS_PARAM: self.me}
        self.assertEqual(self._status(), 404, "default transport is tcp")
        self.values[gate.TRANSPORT_PARAM] = "both"
        self.assertEqual(self._status(), 401)
        self.values[gate.TRANSPORT_PARAM] = "unix"
        self.assertEqual(self._status(), 401)
        status, body = self._version()
        self.assertEqual(status, 200)
        self.assertIn(b"server_version", body)
        tcp = OdooRpcClient(self.base_url(), "not-a-real-api-key", self.env.cr.dbname)
        self.assertEqual(self._status(tcp), 404, "'unix' refuses the endpoints on TCP")

    def test_only_listed_local_users_get_in(self):
        self.values = {gate.TRANSPORT_PARAM: "unix", gate.UNIX_PEERS_PARAM: "somebody_else_hrg, 4000000000"}
        self.assertEqual(self._status(), 404)
        self.assertEqual(self._version()[0], 404)
        self.values[gate.UNIX_PEERS_PARAM] = ""
        self.assertEqual(self._status(), 404)
        self.values[gate.UNIX_PEERS_PARAM] = str(os.getuid())
        self.assertEqual(self._status(), 401)
        self.values[gate.LEGACY_PARAM] = "none"
        self.assertEqual(self._version()[0], 404, "legacy_access none closes the legacy endpoints on the socket too")

    def test_report_mode_serves_a_refused_peer_and_counts_it(self):
        gate._counts.clear()
        self.values = {gate.TRANSPORT_PARAM: "unix", gate.UNIX_PEERS_PARAM: "somebody_else_hrg", policy.MODE_PARAM: "report"}
        self.assertEqual(self._status(), 401)
        self.assertEqual(gate.request_counts()[(gate.FAMILY_JSON2, gate.CLASS_UNIX, "would-refuse:peer")], 1)
        gate._counts.clear()

    def test_only_the_rpc_endpoints_are_served_on_the_socket(self):
        self.values = {gate.TRANSPORT_PARAM: "unix", gate.UNIX_PEERS_PARAM: self.me}
        connection = UnixHTTPConnection(self.path, timeout=10, host=self.host)
        try:
            with self._requests():
                connection.request("GET", "/web/login")
                response = connection.getresponse()
                body = response.read()
            self.assertEqual(response.status, 404)
            self.assertIn(b"only the RPC endpoints", body)
        finally:
            connection.close()

    def test_forwarding_headers_cannot_change_the_identity(self):
        self.values = {gate.TRANSPORT_PARAM: "unix", gate.UNIX_PEERS_PARAM: "somebody_else_hrg"}
        connection = UnixHTTPConnection(self.path, timeout=10, host=self.host)
        try:
            with self._requests():
                connection.request("POST", "/json/2/res.users/context_get", body=b"{}", headers={
                    "Content-Type": "application/json", "Authorization": "Bearer not-a-real-api-key",
                    "X-Odoo-Database": self.env.cr.dbname, "X-Forwarded-For": "10.99.0.4", "X-Real-IP": "10.99.0.4"})
                status = connection.getresponse().status
            self.assertEqual(status, 404)
        finally:
            connection.close()

    def test_the_client_helper_refuses_bad_urls(self):
        for url in ("unix://relative.sock", "ftp://host", "unix:relative"):
            with self.assertRaises(ValueError):
                OdooRpcClient(url, "k", "db")


@tagged("post_install", "-at_install")
class TestUnixLoginBinding(HamsTransactionCase):
    """A local user bound to Odoo logins (`user=login|login`) may act only as those accounts."""
    # Tests [@ANCHOR: hams_rpc_guard:unix_identity]
    # Tests [@ANCHOR: hams_rpc_guard:check_rpc_call]

    def setUp(self):
        super().setUp()
        self.values = {}

        def read(utils, key, default=None):
            value = self.values.get(key)
            return value if value is not None else default
        self.safe_patch_object(ZeroSudoSecurityUtils, "_get_system_param", new=read)
        self.environ = {}
        request = SimpleNamespace(httprequest=SimpleNamespace(environ=self.environ, headers=Headers(),
                                                              path="/json/2/res.partner/search"))
        self.safe_patch_object(guard, "request", new=request)

    def test_identity_is_absent_off_the_socket(self):
        self.assertEqual(guard.unix_identity(), ("", None))

    def test_identity_names_the_local_user(self):
        self.environ[gate.UNIX_PEER_ENVIRON_KEY] = gate.Peer(1, 1001, 1001, "hamsd_fcc", "hamsd").as_dict()
        self.environ[gate.UNIX_LOGINS_ENVIRON_KEY] = frozenset({"callbook_sync"})
        self.assertEqual(guard.unix_identity(), ("unix:hamsd_fcc", {"callbook_sync"}))
        self.environ[gate.UNIX_PEER_ENVIRON_KEY] = gate.Peer(1, 4321, 4321).as_dict()
        self.assertEqual(guard.unix_identity()[0], "unix:uid:4321")

    def test_a_login_that_is_not_bound_is_refused_and_report_mode_only_counts(self):
        partners = self.env["res.partner"].with_user(self.env.ref("base.user_admin"))
        self.environ[gate.UNIX_PEER_ENVIRON_KEY] = gate.Peer(1, 1001, 1001, "hamsd_fcc", "hamsd").as_dict()
        self.environ[gate.UNIX_LOGINS_ENVIRON_KEY] = frozenset({"callbook_sync"})
        with self.assertRaises(AccessError):
            guard.check_rpc_call(partners, "search")
        self.values[policy.MODE_PARAM] = "report"
        guard.check_rpc_call(partners, "search")  # counted, not refused
        counted = [key for key in policy.verdict_counts() if key[3] == "unix-login-not-bound"]
        self.assertTrue(counted)
        self.values[policy.MODE_PARAM] = "enforce"
        self.environ[gate.UNIX_LOGINS_ENVIRON_KEY] = frozenset({"admin"})
        guard.check_rpc_call(partners, "search")

    def test_an_unreadable_parameter_does_not_drop_the_login_binding(self):
        partners = self.env["res.partner"].with_user(self.env.ref("base.user_admin"))
        self.environ[gate.UNIX_PEER_ENVIRON_KEY] = gate.Peer(1, 1001, 1001, "hamsd_fcc", "hamsd").as_dict()
        self.environ[gate.UNIX_LOGINS_ENVIRON_KEY] = frozenset({"callbook_sync"})
        self.values[policy.CLIENT_CERT_HEADER_PARAM] = "X-Client-Cert"
        self.values[gate.TRUSTED_PROXIES_PARAM] = "not-a-network"  # certificate_identity raises ConfigError
        with self.assertLogs(guard._logger, "ERROR"), self.assertRaises(AccessError):
            guard.check_rpc_call(partners, "search")
