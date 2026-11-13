# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""install_server_patches() driven against fake Odoo server classes, so every hook it installs is exercised with its
exact behaviour (mutation run 2026-11-13). The real Odoo classes are never touched."""
import errno
import fcntl
import ipaddress
import os
import shutil
import socket
import tempfile
import time
from types import SimpleNamespace

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_rpc_guard import unix_listener

OPT = unix_listener


class PreforkServer:
    def __init__(self, pid=1):
        self.pid = pid
        self.log = []

    def start(self, *args, **kwargs):
        self.log.append(("start", args, kwargs))
        return "started"

    def fork_and_reload(self):
        self.log.append(("reload", os.environ.get(OPT.INHERITED_FD_ENV)))
        return "reloaded"

    def process_spawn(self):
        self.log.append(("spawn",))
        return "spawned"


class WorkerHTTP:
    def start(self):
        self.log.append("worker_start")

    def sleep(self):
        self.log.append("worker_sleep")
        return "slept"

    def process_work(self):
        self.log.append("worker_work")
        return "worked"


class WorkerCron:
    def start(self):
        self.log.append("cron_start")
        return "cron"


class ThreadedServer:
    def http_spawn(self):
        self.log.append("http_spawn")


def _fake_server_module(running=None):
    return SimpleNamespace(PreforkServer=PreforkServer, WorkerHTTP=WorkerHTTP, WorkerCron=WorkerCron,
                           ThreadedServer=ThreadedServer, server=running, empty_pipe=lambda fd: EMPTIED.append(fd))


EMPTIED = []


@tagged("post_install", "-at_install")
class TestInstallServerPatches(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:install_server_patches]

    def setUp(self):
        super().setUp()
        del EMPTIED[:]
        self.config = {"http_enable": True}
        self.state = {"socket": None, "path": None, "server": None, "patched": False}
        self.module = _fake_server_module()
        self.safe_patch_object(unix_listener, "odoo_server", new=self.module)
        self.safe_patch_object(unix_listener, "config", new=self.config)
        self.safe_patch_object(unix_listener, "_state", new=self.state)
        for cls in (PreforkServer, WorkerHTTP, WorkerCron, ThreadedServer):
            for name, value in list(vars(cls).items()):
                if callable(value) and not name.startswith("__"):
                    self.addCleanup(setattr, cls, name, value)
        self.ensured = []
        self.safe_patch_object(unix_listener, "ensure_listener", new=lambda cfg: self.ensured.append(cfg) or "S")

    def _worker(self, cls=WorkerHTTP, **kw):
        worker = cls()
        worker.log = []
        for key, value in kw.items():
            setattr(worker, key, value)
        return worker

    def test_it_installs_once_and_sets_the_flag(self):
        hooks = ((PreforkServer, "start"), (PreforkServer, "fork_and_reload"), (PreforkServer, "process_spawn"),
                 (WorkerHTTP, "start"), (WorkerHTTP, "sleep"), (WorkerHTTP, "process_work"), (WorkerCron, "start"),
                 (ThreadedServer, "http_spawn"))
        originals = [vars(cls)[name] for cls, name in hooks]
        unix_listener.install_server_patches()
        self.assertTrue(self.state["patched"])
        for (cls, name), original in zip(hooks, originals):
            self.assertIsNot(vars(cls)[name], original, name)
        installed = [vars(cls)[name] for cls, name in hooks]
        unix_listener.install_server_patches()
        self.assertEqual([vars(cls)[name] for cls, name in hooks], installed, "a second call changes nothing")

    def test_an_already_patched_state_installs_nothing(self):
        self.state["patched"] = True
        original = (PreforkServer.start, WorkerHTTP.sleep, ThreadedServer.http_spawn)
        unix_listener.install_server_patches()
        self.assertEqual((PreforkServer.start, WorkerHTTP.sleep, ThreadedServer.http_spawn), original)

    def test_a_worker_that_loads_the_module_first_is_told_why_there_is_no_socket(self):
        self.config[OPT.OPTION_PATH] = " /r/s "
        self.module.server = PreforkServer(pid=os.getpid() + 1)
        with self.assertLogs(unix_listener._logger, "ERROR") as logs:
            unix_listener.install_server_patches()
        self.assertIn("hams_rpc_guard was first loaded in a worker process", logs.records[0].getMessage())
        self.assertIn("server_wide_modules", logs.records[0].getMessage())

    def test_no_complaint_in_the_master_without_a_path_or_with_another_server(self):
        for server, path in ((PreforkServer(pid=os.getpid()), "/r/s"), (PreforkServer(pid=os.getpid() + 1), ""),
                             (PreforkServer(pid=os.getpid() + 1), "   "), (PreforkServer(pid=os.getpid() + 1), None),
                             (object(), "/r/s"), (None, "/r/s")):
            self.state["patched"] = False
            self.module.server = server
            self.config[OPT.OPTION_PATH] = path
            with self.assertNoLogs(unix_listener._logger, "ERROR"):
                unix_listener.install_server_patches()

    def test_prefork_start_opens_the_listener_after_the_original_when_http_is_on(self):
        unix_listener.install_server_patches()
        master = PreforkServer()
        self.assertEqual(master.start(1, a=2), "started")
        self.assertEqual(master.log, [("start", (1,), {"a": 2})])
        self.assertEqual(self.ensured, [self.config])
        self.config["http_enable"] = False
        master.start()
        self.assertEqual(len(self.ensured), 1, "http disabled: no listener")

    def test_prefork_spawn_opens_the_listener_first(self):
        unix_listener.install_server_patches()
        master = PreforkServer()
        order = []
        self.safe_patch_object(unix_listener, "ensure_listener", new=lambda cfg: order.append("ensure"))
        master.log = type("L", (list,), {"append": lambda self, item: (order.append(item[0]), list.append(self, item))[1]})()
        self.assertEqual(master.process_spawn(), "spawned")
        self.assertEqual(order, ["ensure", "spawn"])
        self.config["http_enable"] = False
        order.clear()
        master.process_spawn()
        self.assertEqual(order, ["spawn"])

    def test_fork_and_reload_hands_the_descriptor_to_the_new_master(self):
        unix_listener.install_server_patches()
        master = PreforkServer()
        self.assertEqual(master.fork_and_reload(), "reloaded")
        self.assertEqual(master.log, [("reload", None)], "no listener: nothing handed over")
        sock = socket.socket(socket.AF_UNIX)
        self.addCleanup(sock.close)
        self.addCleanup(os.environ.pop, OPT.INHERITED_FD_ENV, None)
        self.assertTrue(fcntl.fcntl(sock.fileno(), fcntl.F_GETFD) & fcntl.FD_CLOEXEC)
        self.state["socket"] = sock
        master.log.clear()
        self.assertEqual(master.fork_and_reload(), "reloaded")
        self.assertEqual(master.log, [("reload", str(sock.fileno()))], "the variable is set before the original runs")
        self.assertFalse(fcntl.fcntl(sock.fileno(), fcntl.F_GETFD) & fcntl.FD_CLOEXEC, "inheritable across exec")

    def test_http_worker_start_builds_the_server_only_with_a_listener(self):
        unix_listener.install_server_patches()
        made = []
        self.safe_patch_object(unix_listener, "make_wsgi_server", new=lambda sock, app, *a, **k: made.append((sock, app, a, k)) or "SRV")
        worker = self._worker(multi=SimpleNamespace(app="APP"))
        worker.start()
        self.assertEqual(worker.log, ["worker_start"])
        self.assertIsNone(self.state["server"])
        self.assertEqual(made, [])
        self.state["socket"] = "SOCK"
        worker.start()
        self.assertEqual(made, [("SOCK", "APP", (), {})])
        self.assertEqual(self.state["server"], "SRV")
        self.state["socket"] = None
        worker.start()
        self.assertIsNone(self.state["server"], "reset when there is no listener")

    def _listening_pair(self):
        """A listening Unix socket and a TCP listening socket on this host, each with a way to make it readable."""
        directory = tempfile.mkdtemp(prefix="hrg-")
        self.addCleanup(shutil.rmtree, directory, True)
        unix = unix_listener.open_listener(os.path.join(directory, "s.sock"))
        self.addCleanup(unix.close)
        tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.addCleanup(tcp.close)
        tcp.bind((str(ipaddress.IPv4Address(0x7F000001)), 0))
        tcp.listen(1)
        read_end, write_end = os.pipe()
        self.addCleanup(os.close, read_end)
        self.addCleanup(os.close, write_end)
        return unix, tcp, read_end, write_end

    def _slept_for(self, worker):
        started = time.monotonic()
        worker.sleep()
        return time.monotonic() - started

    def test_http_worker_sleep_wakes_for_each_of_the_three_descriptors(self):
        unix_listener.install_server_patches()
        unix, tcp, read_end, write_end = self._listening_pair()
        worker = self._worker(multi=SimpleNamespace(socket=tcp, beat=3), wakeup_fd_r=read_end)
        self.assertEqual(worker.sleep(), "slept")
        self.assertEqual(worker.log, ["worker_sleep"], "no listener: Odoo's own sleep")
        self.state["socket"] = unix
        worker.log.clear()
        worker.multi.beat = 0.2
        self.assertGreaterEqual(self._slept_for(worker), 0.15, "nothing ready: waits out the beat")
        self.assertEqual(worker.log, [], "Odoo's own sleep is not used when there is a listener")
        self.assertEqual(EMPTIED, [read_end], "the wakeup pipe is drained after every wait")
        del EMPTIED[:]
        worker.multi.beat = 5
        client = socket.socket(socket.AF_UNIX)
        self.addCleanup(client.close)
        client.connect(unix.getsockname())
        self.assertLess(self._slept_for(worker), 2, "a connection on the Unix socket wakes the worker")
        self.assertEqual(EMPTIED, [read_end])
        client.close()
        unix.accept()
        other = socket.socket(socket.AF_INET)
        self.addCleanup(other.close)
        other.connect(tcp.getsockname())
        self.assertLess(self._slept_for(worker), 2, "a connection on the TCP socket wakes the worker")
        tcp.accept()
        os.write(write_end, b"x")
        self.assertLess(self._slept_for(worker), 2, "a byte on the wakeup pipe wakes the worker")

    def test_http_worker_sleep_tolerates_only_eintr(self):
        unix_listener.install_server_patches()
        unix, _tcp, read_end, _write = self._listening_pair()
        self.state["socket"] = unix
        failure = []

        class Broken:
            def fileno(self):
                raise OSError(failure[0], "x")
        worker = self._worker(multi=SimpleNamespace(socket=Broken(), beat=1), wakeup_fd_r=read_end)
        failure.append(errno.EINTR)
        self.assertIsNone(worker.sleep())
        self.assertEqual(EMPTIED, [], "an interrupted wait does not drain the pipe")
        failure[0] = errno.EBADF
        with self.assertRaises(OSError) as caught:
            worker.sleep()
        self.assertEqual(caught.exception.args[0], errno.EBADF)

    def test_http_worker_process_work_serves_a_unix_connection(self):
        unix_listener.install_server_patches()
        served = []
        self.safe_patch_object(unix_listener, "serve_unix_connection", new=lambda server, conn: served.append((server, conn)))
        accepted = []

        class Listener:
            def accept(self):
                return accepted.pop()
        worker = self._worker(request_count=3)
        self.assertEqual(worker.process_work(), "worked", "no listener: Odoo's own work")
        self.state.update(socket=Listener(), server="SRV")
        worker.log.clear()
        accepted.append(("CONN", None))
        self.assertIsNone(worker.process_work())
        self.assertEqual(served, [("SRV", "CONN")])
        self.assertEqual(worker.request_count, 4)
        self.assertEqual(worker.log, [], "the TCP accept did not run")
        self.state["server"] = None
        self.assertEqual(worker.process_work(), "worked", "a listener without a server falls back")
        self.assertEqual(served, [("SRV", "CONN")])

    def test_http_worker_process_work_falls_back_on_idle_or_aborted_accepts_and_raises_the_rest(self):
        unix_listener.install_server_patches()
        errors = []

        class Listener:
            def accept(self):
                raise OSError(errors[0], "x")
        self.state.update(socket=Listener(), server="SRV")
        worker = self._worker(request_count=0)
        for code in (errno.EAGAIN, errno.ECONNABORTED):
            errors[:] = [code]
            self.assertEqual(worker.process_work(), "worked", code)
        self.assertEqual(worker.request_count, 0)
        errors[:] = [errno.EMFILE]
        with self.assertRaises(OSError) as caught:
            worker.process_work()
        self.assertEqual(caught.exception.errno, errno.EMFILE)

    def test_cron_worker_start_gives_up_the_listener(self):
        unix_listener.install_server_patches()
        worker = self._worker(WorkerCron)
        self.assertEqual(worker.start(), "cron")
        self.assertEqual(worker.log, ["cron_start"])
        closed = []
        self.state["socket"] = SimpleNamespace(close=lambda: closed.append(True))
        self.assertEqual(worker.start(), "cron")
        self.assertEqual(closed, [True])
        self.assertIsNone(self.state["socket"])

    def test_threaded_http_spawn_serves_the_socket_in_a_daemon_thread(self):
        unix_listener.install_server_patches()
        made, threads = [], []

        class Server:
            def serve_forever(self):
                pass
        server = Server()
        self.safe_patch_object(unix_listener, "make_wsgi_server", new=lambda sock, app, threaded=False: made.append((sock, app, threaded)) or server)

        class Thread:
            def __init__(self, target, name, daemon):
                self.args = (target, name, daemon)
                self.started = False
                threads.append(self)

            def start(self):
                self.started = True
        self.safe_patch_object(unix_listener, "threading", new=SimpleNamespace(Thread=Thread, Lock=unix_listener.threading.Lock))
        spawner = self._worker(ThreadedServer, app="APP")
        spawner.http_spawn()
        self.assertEqual(spawner.log, ["http_spawn"])
        self.assertEqual(self.ensured, [self.config])
        self.assertEqual(made, [("S", "APP", True)])
        self.assertEqual(len(threads), 1)
        self.assertTrue(threads[0].started)
        self.assertEqual(threads[0].args, (server.serve_forever, "hams_rpc_guard.unix_httpd", True))
        self.safe_patch_object(unix_listener, "ensure_listener", new=lambda cfg: None)
        spawner.http_spawn()
        self.assertEqual(len(threads), 1, "no listener: no thread")
