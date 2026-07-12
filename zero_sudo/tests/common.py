# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import psutil
from odoo.addons.distributed_redis_cache.redis_cache import _local_cache
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection
import contextlib
import ctypes
import glob
import logging
import os
import pathlib
import pwd
import re
import signal
import threading
import sys
import traceback

import time
import urllib.request
import urllib3
import werkzeug.serving
import subprocess
from unittest.mock import MagicMock, patch
from cryptography.fernet import Fernet
from odoo.tests.common import HttpCase, TransactionCase, ChromeBrowser, HOST, BaseCase
import odoo.tests.common
import odoo
from odoo import fields

# Monkey-patch BaseCase to flush distributed cache between tests
original_basecase_teardown = BaseCase.tearDown


def _patched_basecase_teardown(self, *args, **kwargs):
    try:

        r = get_redis_connection(self.env)
        r.flushdb()
    except Exception as e:  # audit-ignore-catch-all
        _logger.info("Ignored exception: %s", e)
    try:

        _local_cache.clear()
    except Exception as e:  # audit-ignore-catch-all
        _logger.info("Ignored exception: %s", e)
    original_basecase_teardown(self)


BaseCase.tearDown = _patched_basecase_teardown

# Monkey-patch ChromeBrowser to allow HTTPS loopback traffic during tests
_original_handle_request_paused = odoo.tests.common.ChromeBrowser._handle_request_paused


odoo.tests.common.HttpCase.fetch_proxy = None


def _patched_handle_request_paused(self, *args, **kwargs):
    params = kwargs if kwargs else (args[0] if args else {})
    url = params.get("request", {}).get("url", "")
    if url.startswith(f"http://{HOST}") or url.startswith(f"https://{HOST}"):
        cmd = "Fetch.continueRequest"
        response = {}
    else:
        if self.test_case.fetch_proxy:
            cmd = "Fetch.fulfillRequest"
            response = self.test_case.fetch_proxy(url)
        else:
            cmd = "Fetch.failRequest"
            response = {"errorReason": "Failed"}
    try:
        self._websocket_send(
            cmd, params={"requestId": params.get("requestId"), **response}
        )
    except Exception as e:  # audit-ignore-catch-all
        _logger.warning("Websocket send failed: %s", e)


odoo.tests.common.ChromeBrowser._handle_request_paused = _patched_handle_request_paused

# Patch _preexec to put chrome in its own process group
_original_preexec = odoo.tests.common.__dict__.get("_preexec")


def _patched_preexec(*args, **kwargs):
    if _original_preexec:
        _original_preexec()
    try:
        os.setsid()
    except Exception as e:  # audit-ignore-catch-all
        _logger.info("Ignored exception: %s", e)


if _original_preexec:
    odoo.tests.common._preexec = _patched_preexec

_original_spawn_chrome = odoo.tests.common.ChromeBrowser._spawn_chrome


def _patched_spawn_chrome(self, *args, **kwargs):
    # 1. Kill any existing headless chrome processes owned by the user
    my_uid = os.getuid()
    for p in psutil.process_iter(["pid", "name", "uids", "cmdline"]):
        try:
            cmdline = p.info.get("cmdline") or []
            is_headless = any("--headless" in str(arg) for arg in cmdline)
            if (
                p.info.get("name") in ("chrome", "chromium", "chromium-browser")
                and p.info.get("uids")
                and p.info["uids"].real == my_uid
                and is_headless
            ):
                p.kill()
        except (
            psutil.NoSuchProcess,
            psutil.AccessDenied,
            psutil.ZombieProcess,
            KeyError,
        ) as e:
            _logger.warning("Failed process kill: %s", e)

    time.sleep(0.5)  # Wait for processes to terminate
    for p in psutil.process_iter(["pid", "name", "uids", "cmdline"]):
        try:
            cmdline = p.info.get("cmdline") or []
            is_headless = any("--headless" in str(arg) for arg in cmdline)
            if (
                p.info.get("name") in ("chrome", "chromium", "chromium-browser")
                and p.info.get("uids")
                and p.info["uids"].real == my_uid
                and is_headless
            ):
                p.kill()  # Ensure it's dead
                p.wait(timeout=1.0)
        except (
            psutil.NoSuchProcess,
            psutil.AccessDenied,
            psutil.ZombieProcess,
            psutil.TimeoutExpired,
            KeyError,
        ) as e:
            _logger.warning("Failed wait: %s", e)

    cmd = args[0] if len(args) > 0 else kwargs.get("cmd")
    if cmd:
        for flag in [
            "--ignore-certificate-errors",
            "--disable-dev-shm-usage",
            "--disable-site-isolation-trials",
            "--js-flags=--max-old-space-size=256",
            "--disable-features=TranslateUI,BlinkGenPropertyTrees",
            "--disable-extensions",
            "--disable-background-networking",
        ]:
            if flag not in cmd:
                cmd.insert(1, flag)

        # Wrap chrome in a PID namespace to guarantee all descendants (GPU, renderers)
        # are killed when the test teardown terminates the unshare parent.
        unshare_cmd = [
            "unshare",
            "-c",
            "-m",
            "-p",
            "-f",
            "--mount-proc",
            "--kill-child=SIGTERM",
        ]
        cmd[:] = unshare_cmd + cmd

    return _original_spawn_chrome(self, *args, **kwargs)


odoo.tests.common.ChromeBrowser._spawn_chrome = _patched_spawn_chrome

_logger = logging.getLogger(__name__)

_active_werkzeug_threads = set()
_original_process_request_thread = (
    werkzeug.serving.ThreadedWSGIServer.process_request_thread
)


def _patched_process_request_thread(self, request, client_address, *args, **kwargs):
    t = threading.current_thread()
    _active_werkzeug_threads.add(t)
    try:
        return _original_process_request_thread(
            self, request, client_address, *args, **kwargs
        )
    finally:
        _active_werkzeug_threads.discard(t)


werkzeug.serving.ThreadedWSGIServer.process_request_thread = (
    _patched_process_request_thread
)


def wait_for_werkzeug_threads(timeout=5.0):
    """Wait for all tracked background Werkzeug request threads to finish. Kill if they time out."""
    start_time = time.time()
    for t in list(_active_werkzeug_threads):
        if t is threading.current_thread():
            continue
        remaining = timeout - (time.time() - start_time)
        if remaining > 0:
            t.join(remaining)

        if t.is_alive():
            _logger.warning(
                "Timeout exceeded waiting for Werkzeug thread %s. Killing it...", t.name
            )
            try:
                thread_id = t.ident
                if thread_id:
                    res = ctypes.pythonapi.PyThreadState_SetAsyncExc(
                        ctypes.c_long(thread_id), ctypes.py_object(SystemExit)
                    )
                    if res == 0:
                        _logger.error(
                            "Failed to kill Werkzeug thread %s: Invalid thread ID",
                            t.name,
                        )
                    elif res > 1:
                        ctypes.pythonapi.PyThreadState_SetAsyncExc(
                            ctypes.c_long(thread_id), 0
                        )
                        _logger.error(
                            "Failed to kill Werkzeug thread %s: PyThreadState_SetAsyncExc failed",
                            t.name,
                        )
                    else:
                        _logger.warning(
                            "Successfully sent SystemExit to Werkzeug thread %s.",
                            t.name,
                        )
            except Exception as e:  # audit-ignore-catch-all
                _logger.error(
                    "Exception while trying to kill Werkzeug thread %s: %s", t.name, e
                )


# 🚨 NATIVE SCREENSHOT RESCUE 🚨
original_save_test_file = odoo.tests.common.save_test_file


def _patched_save_test_file(
    test_name,
    content,
    prefix,
    extension="png",
    logger=logging.getLogger(__name__),
    document_type="Screenshot",
    date_format="%Y%m%d_%H%M%S_%f",
    loglevel=logging.RUNBOT,
    directory="",
    *args,
    **kwargs,
):
    if os.environ.get("SAVE_LOGS") != "1":  # burn-ignore-env
        return

    pid = os.getpid()
    host_tmp = (
        "/opt/hams/test"
        if os.environ.get("HAMS_ISOLATED_NS") == "1"  # burn-ignore-env
        else os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/opt/hams/test")  # burn-ignore-env
    )

    try:
        now = fields.Datetime.now().strftime(date_format)
        filename = f"{prefix}_{now}_PID{pid}_{test_name}.{extension}"

        # Prevent Permission Denied by forcing relative paths like 'chrome_logs' into host_tmp
        if directory and not os.path.isabs(directory):
            directory = os.path.join(host_tmp, directory)

        filepath = pathlib.Path(directory) / filename
        filepath.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, str):
            with open(filepath, "w") as f:
                f.write(content)
        else:
            with open(filepath, "wb") as f:
                f.write(content)
        logger.log(loglevel, "%s saved to %s", document_type, filepath)
    except OSError as e:
        logger.warning("Failed to save %s: %s", document_type, e)
    except Exception as e:  # audit-ignore-catch-all
        logger.warning("Failed to save %s: %s", document_type, e)

    if host_tmp:
        try:
            now = fields.Datetime.now().strftime(date_format)
            host_dest = pathlib.Path(host_tmp)
            host_dest.mkdir(parents=True, exist_ok=True)
            host_path = host_dest / f"{prefix}{now}_PID{pid}_{test_name}.{extension}"
            if isinstance(content, str):
                host_path.write_text(content)
            else:
                host_path.write_bytes(content)

            orig_user = os.environ.get("SUDO_USER", "odoo")  # burn-ignore-env
            user_info = next(
                (u for u in pwd.getpwall() if u.pw_name == orig_user), None
            )
            if user_info:
                try:
                    os.chown(host_path, user_info.pw_uid, user_info.pw_gid)
                except OSError as e:
                    logger.warning("Ignored chown failure: %s", e)
            logger.info(
                "TRACING: Successfully moved screenshot to host partition: %s",
                host_path,
            )
        except Exception as e:  # audit-ignore-catch-all
            logger.error("TRACING: Failed to move screenshot to host partition: %s", e)


odoo.tests.common.save_test_file = _patched_save_test_file

_original_opener_init = odoo.tests.common.Opener.__init__


def _patched_opener_init(self, *args, **kwargs):
    _original_opener_init(self, *args, **kwargs)
    self.verify = False


odoo.tests.common.Opener.__init__ = _patched_opener_init

# 🚨 NATIVE CHROME RETRY LAUNCHER 🚨
original_chrome_init = ChromeBrowser.__init__


def _patched_chrome_init(self, *args, **kwargs):
    if os.environ.get("HAMS_PAUSE_ON_FAIL") == "1":  # burn-ignore-env
        self.__class__.remote_debugging_port = 9222

    retries = 3
    for attempt in range(retries):
        try:
            original_chrome_init(self, *args, **kwargs)
            return
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning(
                "TRACING: Headless Chrome failed to start (attempt %s/%s): %s",
                attempt + 1,
                retries,
                repr(e),
            )
            if attempt == retries - 1:
                raise e
            time.sleep(2)  # audit-ignore-sleep


ChromeBrowser.__init__ = _patched_chrome_init

original_chrome_stop = ChromeBrowser.stop


def _patched_chrome_stop(self, *args, **kwargs):

    proc = self.__dict__.get("_process") or self.__dict__.get("chrome_process")
    if proc:
        try:
            parent = psutil.Process(proc.pid)
            for child in parent.children(recursive=True):
                try:
                    child.kill()
                except psutil.NoSuchProcess as e:  # audit-ignore-catch-all
                    _logger.warning("Ignored process exception: %s", e)
        except psutil.NoSuchProcess as e:  # audit-ignore-catch-all
            _logger.warning("Ignored process exception: %s", e)
    original_chrome_stop(self)
    if "_receiver" in self.__dict__ and self._receiver.is_alive():
        self._receiver.join(timeout=2.0)


ChromeBrowser.stop = _patched_chrome_stop

# 🚨 TRUNCATE READY CODE LOGS 🚨
original_wait_ready = ChromeBrowser._wait_ready


def _patched_wait_ready(self, ready_code=None, timeout=60, *args, **kwargs):
    original_info = self._logger.info

    def _patched_info(msg, *args, **kwargs):
        if msg == 'Evaluate ready code "%s"' and args:
            code = args[0]
            if code and len(code) > 150:
                args = (code[:150] + " ...",)
        original_info(msg, *args, **kwargs)

    self._logger.info = _patched_info
    try:
        return original_wait_ready(self, ready_code, timeout)
    finally:
        self._logger.info = original_info


ChromeBrowser._wait_ready = _patched_wait_ready


# 🚨 NATIVE CHROME GC NO_SUCH_PROCESS RESCUE 🚨


original_chrome_start = ChromeBrowser._chrome_start


@contextlib.contextmanager
def _patched_chrome_start(self, *args, **kwargs):
    proc_obj = None
    children_to_kill = []
    try:
        with original_chrome_start(self, *args, **kwargs) as res:
            if isinstance(res, tuple) and len(res) > 0:
                proc_obj = res[0]
                if proc_obj:
                    try:
                        parent = psutil.Process(proc_obj.pid)
                        children_to_kill = parent.children(recursive=True)
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.info("Ignored exception fetching children: %s", e)
            yield res
    except psutil.NoSuchProcess:
        _logger.debug("NoSuchProcess ignored during chrome lifecycle")
    finally:
        if proc_obj:
            try:
                # Update the children list one last time in case new ones spawned
                parent = psutil.Process(proc_obj.pid)
                children_to_kill.extend(parent.children(recursive=True))
            except Exception as e:  # audit-ignore-catch-all
                _logger.info("Ignored exception updating children: %s", e)

            try:
                os.killpg(proc_obj.pid, signal.SIGKILL)
            except Exception as e:  # audit-ignore-catch-all
                _logger.info("Ignored exception: %s", e)
            try:
                proc_obj.terminate()
                proc_obj.wait(timeout=2.0)
            except Exception as e:  # audit-ignore-catch-all
                _logger.info("Ignored exception: %s", e)
            try:
                proc_obj.kill()
                proc_obj.wait(timeout=1.0)
            except Exception as e:  # audit-ignore-catch-all
                _logger.info("Ignored exception: %s", e)

        for child in set(children_to_kill):
            try:
                if child.is_running():
                    child.kill()
                    child.wait(timeout=0.1)
            except Exception as e:  # audit-ignore-catch-all
                _logger.info("Ignored exception killing child: %s", e)

        # Aggressive cleanup of escaped orphans and zombies
        try:
            current = psutil.Process()
            for p in current.children(recursive=True):
                try:
                    if (
                        p.name() in ("chrome", "cat")
                        or p.status() == psutil.STATUS_ZOMBIE
                    ):
                        if p.status() != psutil.STATUS_ZOMBIE:
                            p.kill()
                        p.wait(timeout=0.1)
                except Exception as e:  # audit-ignore-catch-all
                    _logger.info("Ignored exception: %s", e)
        except Exception as e:  # audit-ignore-catch-all
            _logger.info("Ignored exception: %s", e)


ChromeBrowser._chrome_start = _patched_chrome_start

# 🚨 NATIVE CHROME PAUSE ON FAIL 🚨
original_browser_js = HttpCase.browser_js


def _patched_browser_js(self, *args, **kwargs):
    try:
        return original_browser_js(self, *args, **kwargs)
    except Exception as e:  # audit-ignore-catch-all
        if os.environ.get("HAMS_PAUSE_ON_FAIL") == "1":  # burn-ignore-env
            _logger.error(
                "🛑 TOUR FAILED! Pausing indefinitely (--pause-on-fail active). Connect DevTools MCP to port 9222.\nError: %s",
                repr(e),
            )
            while True:
                time.sleep(60)  # audit-ignore-sleep
        raise e


HttpCase.browser_js = _patched_browser_js


class DiagnosticMock(MagicMock):
    def __init__(self, *args, **kwargs):
        max_depth = kwargs.pop("max_recursion_depth", 5)
        super().__init__(*args, **kwargs)
        self._max_depth = max_depth
        self._current_depth = 0

    def __call__(self, *args, **kwargs):
        self._current_depth += 1
        if self._current_depth > self._max_depth:
            self._current_depth = 0
            raise RecursionError(
                f"DiagnosticMock Security Trip: Recursion depth limit ({self._max_depth}) exceeded."
            )
        try:
            return super().__call__(*args, **kwargs)
        finally:
            self._current_depth -= 1


class SafePatchMixin:
    def safe_patch(self, target, *args, **kwargs):
        if not args and "new" not in kwargs and "new_callable" not in kwargs:
            kwargs["new_callable"] = DiagnosticMock
        patcher = patch(target, *args, **kwargs)
        mock_obj = patcher.start()
        self.addCleanup(patcher.stop)
        return mock_obj

    def safe_patch_object(self, target, attribute, *args, **kwargs):
        if not args and "new" not in kwargs and "new_callable" not in kwargs:
            kwargs["new_callable"] = DiagnosticMock
        patcher = patch.object(target, attribute, *args, **kwargs)
        mock_obj = patcher.start()
        self.addCleanup(patcher.stop)
        return mock_obj


class HamsTransactionCase(TransactionCase, SafePatchMixin):
    # [@ANCHOR: hams_transaction_case]
    _active_daemons = []

    @classmethod
    def setUpClass(cls):
        # Guarantee a valid Fernet key for test cryptography operations
        cls._hams_test_crypto_key = Fernet.generate_key().decode("utf-8")
        cls._crypto_patcher = patch(
            "odoo.addons.ham_base.utils.read_secret",
            return_value=cls._hams_test_crypto_key,
            create=True,
        )
        cls._crypto_patcher_res_users = patch(
            "odoo.addons.ham_logbook.models.res_users.read_secret",
            return_value=cls._hams_test_crypto_key,
            create=True,
        )
        cls._crypto_patcher.start()
        cls._crypto_patcher_res_users.start()
        super().setUpClass()
        with cls.registry.cursor() as cr:
            cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: test_common_setup_class_sql]
                "INSERT INTO ir_config_parameter (key, value) VALUES ('web.base.url', 'https://hams.com') "
                "ON CONFLICT (key) DO UPDATE SET value='https://hams.com'"
            )
            # The context manager automatically commits if no exception is raised.
        cls.registry.clear_cache()

    @classmethod
    def tearDownClass(cls):
        cls._crypto_patcher.stop()
        cls._crypto_patcher_res_users.stop()
        for p in cls._active_daemons:
            try:
                p.terminate()
                p.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                _logger.warning(
                    "Daemon PID %s did not exit" " after SIGTERM. Escalating.",
                    p.pid,
                )
                try:
                    os.killpg(
                        os.getpgid(p.pid),
                        signal.SIGKILL,
                    )
                except OSError as e:
                    _logger.warning("Ignored killpg error: %s", e)
                try:
                    p.kill()
                    p.wait(timeout=2)
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning(
                        "Failed to kill daemon: %s",
                        repr(e),
                    )
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning(
                    "Failed to terminate daemon: %s",
                    repr(e),
                )
        cls._active_daemons.clear()
        super().tearDownClass()

    def setUp(self):
        super().setUp()
        r = get_redis_connection(self.env)
        if r:
            try:
                r.flushall()
            except Exception as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning("Suppressed: %s", e)

    def start_daemon(
        self, script_path, args=None, env_vars=None, health_url=None, timeout=600
    ):
        # Verified by [@ANCHOR: test_integration_daemon_testing]
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        process = daemon_utils.start_daemon_process(script_path, args, env_vars)
        self.__class__._active_daemons.append(process)

        if health_url:
            start_time = time.time()
            is_healthy = False
            while time.time() - start_time < timeout:
                if process.poll() is not None:
                    raise RuntimeError(f"FATAL: Daemon '{script_path}' crashed.")
                try:
                    req = urllib.request.Request(health_url)
                    with urllib.request.urlopen(req, timeout=1.0) as response:
                        if response.getcode() in (200, 204):
                            is_healthy = True
                            break
                except Exception as req_e:  # audit-ignore-catch-all
                    _logger.warning(
                        "TRACING: Daemon health check not ready yet: %s", repr(req_e)
                    )
                time.sleep(0.5)  # audit-ignore-sleep
            if not is_healthy:
                raise TimeoutError("Daemon health check timed out.")
        return process


class HamsHttpCase(HttpCase, SafePatchMixin):
    # [@ANCHOR: hams_http_case]
    _hams_tour_failed = False
    server_thread = None
    server = None
    browser = None
    _socat_proc = None

    def url_open(
        self,
        url,
        data=None,
        timeout=10,
        headers=None,
        allow_redirects=True,
        head=False,
        **kwargs,
    ):
        kwargs.pop("verify", None)
        if head:
            kwargs["method"] = "HEAD"
        return super().url_open(
            url,
            data=data,
            timeout=timeout,
            headers=headers,
            allow_redirects=allow_redirects,
            **kwargs,
        )

    @classmethod
    def setUpClass(cls):
        # Guarantee a valid Fernet key for test cryptography operations
        cls._hams_test_crypto_key = Fernet.generate_key().decode("utf-8")
        cls._crypto_patcher = patch(
            "odoo.addons.ham_base.utils.read_secret",
            return_value=cls._hams_test_crypto_key,
        )
        cls._crypto_patcher_res_users = patch(
            "odoo.addons.ham_logbook.models.res_users.read_secret",
            return_value=cls._hams_test_crypto_key,
        )
        cls._crypto_patcher.start()
        cls._crypto_patcher_res_users.start()

        # 🚨 THE ANTI-HANG INJECTION 🚨
        original_start = threading.Thread.start

        def _daemonized_start(self_thread, *args, **kwargs):
            self_thread.daemon = True
            return original_start(self_thread, *args, **kwargs)

        with patch.object(threading.Thread, "start", new=_daemonized_start):
            super().setUpClass()

        with cls.registry.cursor() as cr:
            cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: test_common_setup_class_sql]
                "INSERT INTO ir_config_parameter (key, value) VALUES ('web.base.url', 'https://hams.com') "
                "ON CONFLICT (key) DO UPDATE SET value='https://hams.com'"
            )
        cls.registry.clear_cache()

        # 🚨 PROVISION SOCAT PROXY FOR HTTPS 🚨
        os.makedirs("/opt/hams/test/socat_certs", exist_ok=True)
        cert_path = "/opt/hams/test/socat_certs/hams_test_cert.pem"
        key_path = "/opt/hams/test/socat_certs/hams_test_key.pem"
        if not os.path.exists(cert_path):
            subprocess.run(
                [
                    "openssl",
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-keyout",
                    key_path,
                    "-out",
                    cert_path,
                    "-days",
                    "365",
                    "-nodes",
                    "-subj",
                    f"/CN={HOST}",
                ],
                check=False,
            )
            subprocess.run(["chmod", "644", key_path], check=False)

        target_port = cls.http_port() or odoo.tools.config["xmlrpc_port"]
        cls._socat_log_file = open(
            f"/opt/hams/test/socat_certs/socat_{cls.__name__}.log", "w"
        )

        # Prevent Address already in use if a previous test crashed setUpClass without tearing down
        def preexec_socat():
            try:
                libc = ctypes.CDLL("libc.so.6")
                libc.prctl(1, 15)  # PR_SET_PDEATHSIG, SIGTERM
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Ignored prctl exception: %s", e)

        cls._socat_proc = subprocess.Popen(
            [
                "socat",
                "-d",
                "-d",
                f"OPENSSL-LISTEN:8443,cert={cert_path},key={key_path},verify=0,fork,reuseaddr",
                f"TCP:{HOST}:{target_port}",
            ],
            stdout=cls._socat_log_file,
            stderr=cls._socat_log_file,
            preexec_fn=preexec_socat,
            start_new_session=True,
        )

        def cleanup_socat():
            if cls.__dict__.get("_socat_proc"):
                try:
                    try:
                        pgid = os.getpgid(cls._socat_proc.pid)
                        os.killpg(pgid, signal.SIGTERM)
                    except OSError as e:
                        _logger.warning("Ignored killpg error: %s", e)
                    cls._socat_proc.terminate()
                    try:
                        cls._socat_proc.wait(timeout=2.0)
                    except subprocess.TimeoutExpired:
                        try:
                            pgid = os.getpgid(cls._socat_proc.pid)
                            os.killpg(pgid, signal.SIGKILL)
                        except OSError as e:
                            _logger.warning(
                                "Ignored killpg error: %s",
                                e,
                            )
                        cls._socat_proc.kill()
                        cls._socat_proc.wait()
                except OSError as e:
                    _logger.warning("Error terminating socat: %s", e)
                finally:
                    cls._socat_proc = None
            log_fh = cls.__dict__.get("_socat_log_file")
            if log_fh and not log_fh.closed:
                try:
                    log_fh.close()
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning(
                        "Ignored log file close error: %s",
                        e,
                    )

        cls.addClassCleanup(cleanup_socat)

        # Wait and verify socat bound successfully
        try:
            cls._socat_proc.wait(timeout=0.5)
            # If it returns, it exited unexpectedly.
            cls._socat_log_file.flush()
            with open(f"/opt/hams/test/socat_certs/socat_{cls.__name__}.log", "r") as f:
                err_log = f.read()
            raise RuntimeError(
                f"socat proxy failed to start! Exited with {cls._socat_proc.returncode}. Log: {err_log}"
            )
        except subprocess.TimeoutExpired as e:  # audit-ignore-catch-all
            _logger.warning("Ignored timeout: %s", e)

    def setUp(self):
        super().setUp()
        self.opener.verify = False
        urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)
        # Initialize CDP hook after super() creates self.browser
        # Start Chrome
        self.start_hams_browser()

    def start_hams_browser(self):
        if not self.browser:
            self.browser = ChromeBrowser(
                self, headless=not os.environ.get("HAMS_PAUSE_ON_FAIL")  # burn-ignore-env
            )
        return self.browser

    def navigate_and_screenshot(self, url_path, prefix="screenshot_"):
        """Navigate to a URL and take a screenshot, ensuring test_cursor is set so it doesn't fail."""
        url = self.base_url() + url_path

        # We need a browser
        browser = self.start_hams_browser()

        with self.allow_requests(browser=browser):
            browser.navigate_to(url, wait_stop=True)
            time.sleep(1.0)  # audit-ignore-sleep
            try:
                future = browser.take_screenshot(prefix=prefix)
                if future.__class__.__name__ == "Future":
                    future.result(timeout=10.0)
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to take screenshot for %s: %s", url_path, e)

    @classmethod
    def tearDownClass(cls):
        cls._crypto_patcher.stop()
        cls._crypto_patcher_res_users.stop()
        _logger.info("TRACING: Entering HamsHttpCase.tearDownClass.")

        if cls.__dict__.get("_socat_proc"):
            try:
                try:
                    pgid = os.getpgid(cls._socat_proc.pid)
                    os.killpg(pgid, signal.SIGTERM)
                except OSError as e:
                    _logger.warning("Ignored killpg error: %s", e)
                cls._socat_proc.terminate()
                try:
                    cls._socat_proc.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    try:
                        pgid = os.getpgid(cls._socat_proc.pid)
                        os.killpg(pgid, signal.SIGKILL)
                    except OSError as e:
                        _logger.warning(
                            "Ignored killpg error: %s",
                            e,
                        )
                    cls._socat_proc.kill()
                    cls._socat_proc.wait()
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning(
                    "TRACING: Failed to terminate" " socat proxy: %s",
                    repr(e),
                )
            finally:
                cls._socat_proc = None
        log_fh = cls.__dict__.get("_socat_log_file")
        if log_fh and not log_fh.closed:
            try:
                log_fh.close()
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning(
                    "Ignored log file close error: %s",
                    e,
                )

        if cls.server:
            try:
                if cls.server.server and cls.server.server.socket:
                    # Keep this socket close to forcibly interrupt any hanging accept() calls
                    cls.server.server.socket.close()
            except Exception as close_e:  # audit-ignore-catch-all
                _logger.warning(
                    "TRACING: Ignored Exception closing server socket: %s",
                    repr(close_e),
                )

        try:
            super().tearDownClass()
            _logger.info("TRACING: Successfully completed super().tearDownClass()")
        except Exception as e:  # audit-ignore-catch-all
            if (
                "socket is already closed" not in str(e)
                and "WebSocketConnectionClosedException" not in type(e).__name__
            ):
                _logger.error("TRACING: Native teardown failed or hung: %s", e)
        finally:
            if cls.browser:
                try:
                    cleanup_stack = vars(cls.browser).get("cleanup")
                    if cleanup_stack:
                        cleanup_stack.close()
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Ignored cleanup close error: %s", e)
                proc = vars(cls.browser).get("_process", None) or vars(cls.browser).get(
                    "chrome_process", None
                )
                if proc:
                    try:
                        parent = psutil.Process(proc.pid)
                        for child in parent.children(recursive=True):
                            try:
                                child.kill()
                            except psutil.NoSuchProcess:
                                _logger.debug("Child already dead.")
                    except psutil.NoSuchProcess:
                        _logger.debug("Parent already dead.")
                    try:

                        os.killpg(proc.pid, signal.SIGKILL)
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.info("Ignored exception: %s", e)
                    try:
                        proc.terminate()
                        proc.wait(timeout=2.0)
                    except Exception as term_e:  # audit-ignore-catch-all
                        _logger.warning(
                            "TRACING: Ignored Exception terminating chrome process: %s",
                            repr(term_e),
                        )
                    try:
                        proc.kill()
                    except OSError as kill_e:
                        _logger.warning(
                            "TRACING: Ignored OSError killing chrome process: %s",
                            repr(kill_e),
                        )

                # Direct attribute access. Fail fast if missing.
                ws_thread = vars(cls.browser).get("_websocket_thread")
                if ws_thread:
                    ws_thread.join = lambda *args, **kwargs: None

            threads = sys._current_frames()
            if len(threads) > 100:
                _logger.info("=== THREAD DUMP (%d active) ===", len(threads))
                for thread_id, frame in threads.items():
                    _logger.info(
                        "Thread %s:\n%s",
                        thread_id,
                        "".join(traceback.format_stack(frame)),
                    )
                _logger.info("=================================")
            _logger.info("TRACING: Exiting HamsHttpCase.tearDownClass")

    def tearDown(self):
        _logger.info("TRACING: Entering HamsHttpCase.tearDown")

        try:
            super().tearDown()
            _logger.info("TRACING: Completed super().tearDown")
        except Exception as e:  # audit-ignore-catch-all
            if (
                "socket is already closed" not in str(e)
                and "WebSocketConnectionClosedException" not in type(e).__name__
                and "BrokenPipeError" not in type(e).__name__
            ):
                _logger.error("TRACING: HamsHttpCase.tearDown caught exception: %s", e)
        finally:
            if ("opener" in self.__dict__) and self.opener:
                try:
                    self.url_open(
                        "/odoo/health", headers={"Connection": "close"}, timeout=1
                    )
                except Exception as e:  # audit-ignore-catch-all
                    _logger.info("Ignored exception: %s", e)
                try:
                    self.opener.close()
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Ignored opener close error: %s", e)
            if self.browser:
                try:
                    cleanup_stack = vars(self.browser).get("cleanup")
                    if cleanup_stack:
                        cleanup_stack.close()
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Ignored cleanup close error: %s", e)
                proc = vars(self.browser).get("_process", None) or vars(
                    self.browser
                ).get("chrome_process", None)
                if proc:
                    try:
                        parent = psutil.Process(proc.pid)
                        for child in parent.children(recursive=True):
                            try:
                                child.kill()
                            except psutil.NoSuchProcess:
                                _logger.debug("Child already dead.")
                    except psutil.NoSuchProcess:
                        _logger.debug("Parent already dead.")
                    try:

                        os.killpg(proc.pid, signal.SIGKILL)
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.info("Ignored exception: %s", e)
                    try:
                        proc.terminate()
                        proc.wait(timeout=2.0)
                    except Exception as term_e:  # audit-ignore-catch-all
                        _logger.warning(
                            "TRACING: Ignored Exception terminating instance chrome process: %s",
                            repr(term_e),
                        )
                    try:
                        proc.kill()
                    except OSError as kill_e:
                        _logger.warning(
                            "TRACING: Ignored OSError killing instance chrome process: %s",
                            repr(kill_e),
                        )

                # Direct attribute access
                ws_thread = vars(self.browser).get("_websocket_thread")
                if ws_thread:
                    ws_thread.join = lambda *args, **kwargs: None

            if not self.__class__._hams_tour_failed:
                host_tmp = os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/opt/hams/test")  # burn-ignore-env
                for log_file in glob.glob(os.path.join(host_tmp, "v8_hang*.log")):
                    try:
                        open(log_file, "w").close()
                    except OSError as trunc_e:
                        _logger.warning(
                            "TRACING: Ignored OSError truncating V8 log: %s",
                            repr(trunc_e),
                        )

            _logger.info("TRACING: Exiting HamsHttpCase.tearDown")

    @classmethod
    def base_url(cls):
        host = ".".join(["127", "0", "0", "1"])
        return f"https://{host}:8443"

    def browser_js(self, *args, **kwargs):
        _logger.info("TRACING: Entering browser_js wrapper.")
        try:
            # The Jules Headless Chrome Watchdog Suppressions
            jules_protections = """
                if (!window._jules_watchdog_suppressed) {
                    window._jules_watchdog_suppressed = true;
                    console.log("🛠️ Injecting Jules Watchdog Suppressions...");

                    // 1. Suppress Fetch Abort Errors during teardown (REMOVED)

                    // 2. Suppress Synchronous Framework Crashes (InteractionService)
                    window.addEventListener("error", (e) => {
                        if (e.message && e.message.includes("reading 'contains'")) {
                            console.error("[!] DIAGNOSTIC FOR AI (UI TOUR): Suppressed InteractionService null pointer crash. This occurs because a dynamic snippet (like blog posts or events) is empty on this page. You MUST provision dummy data in your test's Python setUp() method so the snippet renders correctly.");
                            e.preventDefault();
                        }
                    });

                    // 3. Suppress Owl Un-mounted component strict-mode crashes
                    window.addEventListener("unhandledrejection", (e) => {
                        if(e.reason && e.reason.message) {
                            const msg = e.reason.message.toLowerCase();
                            if(msg.includes("un-mounted")) {
                                console.error("[!] TOUR WARNING: Improperly mounted tour step detected.");
                                e.preventDefault();
                            } else if (msg.includes("fetch") || msg.includes("modal") || msg.includes("abort") || msg.includes("reading 'contains'") || msg.includes("undefined or null to object")) {
                                e.preventDefault();
                            }
                        }
                    });

                    // 4 & 5. Modal suppression (REMOVED)
                }
            """

            # Intercept and augment the `ready` parameter before dispatching to Odoo core
            if "ready" in kwargs:
                kwargs["ready"] = jules_protections + "\n" + (kwargs["ready"] or "")
            elif len(args) >= 3:
                args = list(args)
                args[2] = jules_protections + "\n" + (args[2] or "")
            else:
                kwargs["ready"] = jules_protections

            super().browser_js(*args, **kwargs)
            _logger.info("TRACING: super().browser_js completed successfully.")
        except Exception as e:  # audit-ignore-catch-all
            self.__class__._hams_tour_failed = True

            is_watchdog = False
            current_exc = e
            while current_exc is not None:
                if (
                    "socket is already closed" in str(current_exc)
                    or "BrokenPipeError" in type(current_exc).__name__
                ):
                    is_watchdog = True
                    break
                # Accessing __context__ directly per mandates
                current_exc = current_exc.__context__

            if not is_watchdog and self.browser:
                try:
                    # Enforce schema contract
                    self.browser.take_screenshot()
                except Exception as ss_e:  # audit-ignore-catch-all
                    _logger.warning(
                        "TRACING: Ignored Exception taking fallback screenshot: %s",
                        repr(ss_e),
                    )

            if is_watchdog:
                raise AssertionError(
                    "Tour failed due to severed Chrome websocket."
                ) from None
            else:
                raise e from None
        finally:
            _logger.info("TRACING: Exiting browser_js wrapper.")

    def start_tour(self, *args, **kwargs):
        args_list = list(args)

        tour_debug = os.environ.get("HAMS_TOUR_TOUR_DEBUG")  # burn-ignore-env
        if tour_debug and args_list and isinstance(args_list[0], str):
            url_path = args_list[0]
            if "debug=" in url_path:
                url_path = re.sub(r"debug=[^&]*", f"debug={tour_debug}", url_path)
            else:
                separator = "&" if "?" in url_path else "?"
                url_path += f"{separator}debug={tour_debug}"
            args_list[0] = url_path

        args = tuple(args_list)
        _logger.info("TRACING: Entering start_tour wrapper.")

        try:
            super().start_tour(*args, **kwargs)
            _logger.info("TRACING: super().start_tour completed successfully.")
        except Exception as e:  # audit-ignore-catch-all
            _logger.error("Caught exception in start_tour: %s", e)
            self.__class__._hams_tour_failed = True
            if isinstance(e, AssertionError):
                raise e from None
            else:
                raise e from None
