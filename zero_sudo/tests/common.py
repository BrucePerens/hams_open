# -*- coding: utf-8 -*-
import glob
import logging
import os
import pwd
import re
import shutil
import time
import urllib.request
import threading
import json
import unittest
from unittest.mock import MagicMock, patch
from odoo.tests.common import HttpCase, TransactionCase, ChromeBrowser
from . import watchdog_shared

_logger = logging.getLogger(__name__)

def _apply_cdp_hook(browser_instance):
    if not browser_instance:
        return
    watchdog_shared.global_captured_stack = None
    watchdog_shared.global_active_browser = browser_instance
    if hasattr(browser_instance, '_websocket') and browser_instance._websocket:
        if not hasattr(browser_instance._websocket.recv, '_is_hams_patched'):
            original_recv = browser_instance._websocket.recv
            def _intercepted_recv(*r_args, **r_kwargs):
                msg = original_recv(*r_args, **r_kwargs)
                if isinstance(msg, str) and 'Debugger.paused' in msg:
                    try:
                        data = json.loads(msg)
                        frames = data.get('params', {}).get('callFrames', [])
                        stack_lines = ["🚨 V8 HUNG THREAD STACK TRACE 🚨"]
                        for f in frames:
                            func = f.get('functionName', '') or '(anonymous)'
                            url = f.get('url', '') or 'unknown'
                            loc = f.get('location', {})
                            line = loc.get('lineNumber', 0) + 1
                            col = loc.get('columnNumber', 0) + 1
                            stack_lines.append(f"    at {func} ({url}:{line}:{col})")
                        watchdog_shared.global_captured_stack = "\n".join(stack_lines)
                        _logger.error("Successfully captured V8 stack trace via CDP!")
                    except Exception as e: # audit-ignore-catch-all
                        _logger.error("Failed to parse Debugger.paused event: %s", e)
                return msg
            _intercepted_recv._is_hams_patched = True
            browser_instance._websocket.recv = _intercepted_recv

if hasattr(ChromeBrowser, 'start'):
    original_browser_start = ChromeBrowser.start
    def _patched_start(self, *args, **kwargs):
        for attempt in range(4):
            try:
                res = original_browser_start(self, *args, **kwargs)
                _apply_cdp_hook(self)
                return res
            except (Exception, unittest.SkipTest) as e:
                if attempt == 3:
                    raise
                _logger.warning("TRACING: Chrome start failed on attempt %d (%s). Retrying...", attempt + 1, e)
                if hasattr(self, 'chrome_process') and self.chrome_process:
                    try:
                        self.chrome_process.terminate()
                        self.chrome_process.wait(timeout=1.0)
                    except OSError as te:
                        _logger.warning("TRACING: Failed terminating chrome process: %s", te)
                time.sleep(1.0) # audit-ignore-sleep
    ChromeBrowser.start = _patched_start
else:
    original_browser_init = ChromeBrowser.__init__
    def _patched_init(self, *args, **kwargs):
        for attempt in range(4):
            try:
                original_browser_init(self, *args, **kwargs)
                _apply_cdp_hook(self)
                break
            except (Exception, unittest.SkipTest) as e:
                if attempt == 3:
                    raise
                _logger.warning("TRACING: Chrome init failed on attempt %d (%s). Retrying...", attempt + 1, e)
                if hasattr(self, 'chrome_process') and self.chrome_process:
                    try:
                        self.chrome_process.terminate()
                        self.chrome_process.wait(timeout=1.0)
                    except OSError as te:
                        _logger.warning("TRACING: Failed terminating chrome process: %s", te)
                time.sleep(1.0) # audit-ignore-sleep
    ChromeBrowser.__init__ = _patched_init

_original_time = time.time

class PassiveVirtualClock:
    """
    A CPU-time equivalent clock that suppresses massive jumps in wall-clock time
    caused by the VM being suspended, entirely without GIL-thrashing threads.
    """
    def __init__(self):
        self.vtime = _original_time()
        self.last_real = _original_time()
        self._lock = threading.RLock()

    def time(self):
        with self._lock:
            now = _original_time()
            delta = now - self.last_real
            self.last_real = now
            self.vtime += min(delta, 0.5)
            return self.vtime

global_vclock = PassiveVirtualClock()

# 🚨 INJECT VIRTUAL CLOCK INTO ODOO CHROME BROWSER ENGINE 🚨
if hasattr(ChromeBrowser, '_wait_ready'):
    original_wait_ready = ChromeBrowser._wait_ready
    def _patched_wait_ready(self, ready_code, timeout=60, *args, **kwargs):
        with patch('time.time', side_effect=global_vclock.time):
            return original_wait_ready(self, ready_code, timeout=timeout, *args, **kwargs)
    ChromeBrowser._wait_ready = _patched_wait_ready

if hasattr(ChromeBrowser, '_wait_code_ok'):
    original_wait_code_ok = ChromeBrowser._wait_code_ok
    def _patched_wait_code_ok(self, code, timeout, *args, **kwargs):
        with patch('time.time', side_effect=global_vclock.time):
            return original_wait_code_ok(self, code, timeout, *args, **kwargs)
    ChromeBrowser._wait_code_ok = _patched_wait_code_ok

# 🚨 BENIGN ERROR SCRUBBER 🚨
if hasattr(ChromeBrowser, 'stop'):
    original_browser_stop = ChromeBrowser.stop
    def _patched_browser_stop(self, *args, **kwargs):
        for attr in ['_errors', '_browser_errors', 'errors']:
            if hasattr(self, attr):
                error_list = getattr(self, attr)
                if isinstance(error_list, list):
                    filtered = []
                    for e in error_list:
                        msg = str(e).lower()
                        if "owl is running in 'dev' mode" in msg or "resizeobserver" in msg:
                            continue
                        filtered.append(e)
                    setattr(self, attr, filtered)
        return original_browser_stop(self, *args, **kwargs)
    ChromeBrowser.stop = _patched_browser_stop

# 🚨 NATIVE SCREENSHOT RESCUE 🚨
if hasattr(ChromeBrowser, 'take_screenshot'):
    original_take_screenshot = ChromeBrowser.take_screenshot
    def _patched_take_screenshot(self, *args, **kwargs):
        try:
            path = original_take_screenshot(self, *args, **kwargs)
            if hasattr(path, 'result'):
                path = path.result(timeout=20.0)

            _logger.error("TRACING: Screenshot generated by Chrome at %s", path)

            host_tmp = os.environ.get("HAMS_REAL_LOG_DIRECTORY")
            if path and os.path.exists(path) and host_tmp:
                os.makedirs(host_tmp, exist_ok=True)

                orig_user = os.environ.get("SUDO_USER", "odoo")
                orig_uid, orig_gid = -1, -1
                try:
                    user_info = pwd.getpwnam(orig_user)
                    orig_uid = user_info.pw_uid
                    orig_gid = user_info.pw_gid
                    if not os.path.exists(host_tmp):
                        os.chown(host_tmp, orig_uid, orig_gid)
                except Exception as user_e: # audit-ignore-catch-all
                    _logger.warning("TRACING: Could not resolve SUDO_USER info for chown: %s", repr(user_e))

                dst = os.path.join(host_tmp, os.path.basename(path))
                shutil.copy2(path, dst)
                try:
                    if orig_uid != -1:
                        os.chown(dst, orig_uid, orig_gid)
                except Exception as chown_e: # audit-ignore-catch-all
                    _logger.warning("TRACING: Could not chown screenshot file: %s", repr(chown_e))

                _logger.error("TRACING: Successfully moved screenshot to host partition: %s", dst)
            return path
        except Exception as ss_e: # audit-ignore-catch-all
            _logger.warning("TRACING: Native screenshot failed: %s", repr(ss_e))
    ChromeBrowser.take_screenshot = _patched_take_screenshot

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

    @classmethod
    def tearDownClass(cls):
        if hasattr(cls, '_active_daemons'):
            for p in cls._active_daemons:
                try:
                    p.terminate()
                    p.wait(timeout=2.0)
                except Exception as e: # audit-ignore-catch-all
                    _logger.warning("Failed to terminate daemon: %s", repr(e))
            cls._active_daemons.clear()
        super().tearDownClass()

    def start_daemon(self, script_path, args=None, env_vars=None, health_url=None, timeout=600):
        if not hasattr(self.__class__, '_active_daemons'):
            self.__class__._active_daemons = []
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        process = daemon_utils.start_daemon_process(script_path, args, env_vars)
        self.__class__._active_daemons.append(process)

        if health_url:
            start_time = global_vclock.time()
            is_healthy = False
            while global_vclock.time() - start_time < timeout:
                if process.poll() is not None:
                    raise RuntimeError(f"FATAL: Daemon '{script_path}' crashed.")
                try:
                    req = urllib.request.Request(health_url)
                    with urllib.request.urlopen(req, timeout=1.0) as response:
                        if response.getcode() in (200, 204):
                            is_healthy = True
                            break
                except Exception as req_e: # audit-ignore-catch-all
                    _logger.warning("TRACING: Daemon health check not ready yet: %s", repr(req_e))
                time.sleep(0.5)  # audit-ignore-sleep
            if not is_healthy:
                raise TimeoutError("Daemon health check timed out.")
        return process

class HamsHttpCase(HttpCase, SafePatchMixin):
    # [@ANCHOR: hams_http_case]

    @classmethod
    def setUpClass(cls):
        # 🚨 THE ANTI-HANG INJECTION 🚨
        original_start = threading.Thread.start

        def _daemonized_start(self_thread, *args, **kwargs):
            self_thread.daemon = True
            return original_start(self_thread, *args, **kwargs)

        with patch.object(threading.Thread, 'start', new=_daemonized_start):
            super().setUpClass()

    @classmethod
    def tearDownClass(cls):
        _logger.info("TRACING: Entering HamsHttpCase.tearDownClass.")

        if hasattr(cls, 'server_thread') and cls.server_thread:
            cls.server_thread.join = lambda *args, **kwargs: None

        if hasattr(cls, 'server') and cls.server:
            try:
                if hasattr(cls.server, 'server') and hasattr(cls.server.server, 'socket'):
                    cls.server.server.socket.close()
            except Exception as close_e: # audit-ignore-catch-all
                _logger.warning("TRACING: Ignored Exception closing server socket: %s", repr(close_e))
            cls.server.stop = lambda *args, **kwargs: None

        try:
            super().tearDownClass()
            _logger.info("TRACING: Successfully completed super().tearDownClass()")
        except Exception as e: # audit-ignore-catch-all
            if "socket is already closed" not in str(e) and "WebSocketConnectionClosedException" not in type(e).__name__:
                _logger.error("TRACING: Native teardown failed or hung: %s", e)
        finally:
            if hasattr(cls, 'browser') and cls.browser:
                if hasattr(cls.browser, 'chrome_process'):
                    try:
                        cls.browser.chrome_process.terminate()
                        cls.browser.chrome_process.wait(timeout=2.0)
                    except Exception as term_e: # audit-ignore-catch-all
                        _logger.warning("TRACING: Ignored Exception terminating chrome process: %s", repr(term_e))
                    try:
                        cls.browser.chrome_process.kill()
                    except OSError as kill_e:
                        _logger.warning("TRACING: Ignored OSError killing chrome process: %s", repr(kill_e))
                if hasattr(cls.browser, '_websocket_thread') and cls.browser._websocket_thread:
                    cls.browser._websocket_thread.join = lambda *args, **kwargs: None

            _logger.info("TRACING: Exiting HamsHttpCase.tearDownClass")

    def tearDown(self):
        _logger.info("TRACING: Entering HamsHttpCase.tearDown")

        try:
            super().tearDown()
            _logger.info("TRACING: Completed super().tearDown")
        except Exception as e: # audit-ignore-catch-all
            if "socket is already closed" not in str(e) and "WebSocketConnectionClosedException" not in type(e).__name__ and "BrokenPipeError" not in type(e).__name__:
                _logger.error("TRACING: HamsHttpCase.tearDown caught exception: %s", e)
        finally:
            if hasattr(self, 'browser') and self.browser:
                if hasattr(self.browser, 'chrome_process'):
                    try:
                        self.browser.chrome_process.terminate()
                        self.browser.chrome_process.wait(timeout=2.0)
                    except Exception as term_e: # audit-ignore-catch-all
                        _logger.warning("TRACING: Ignored Exception terminating instance chrome process: %s", repr(term_e))
                    try:
                        self.browser.chrome_process.kill()
                    except OSError as kill_e:
                        _logger.warning("TRACING: Ignored OSError killing instance chrome process: %s", repr(kill_e))
                if hasattr(self.browser, '_websocket_thread') and self.browser._websocket_thread:
                    self.browser._websocket_thread.join = lambda *args, **kwargs: None

            if not getattr(self.__class__, '_hams_tour_failed', False):
                host_tmp = os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/var/tmp")
                for log_file in glob.glob(os.path.join(host_tmp, "v8_hang*.log")):
                    try:
                        open(log_file, 'w').close()
                    except OSError as trunc_e:
                        _logger.warning("TRACING: Ignored OSError truncating V8 log: %s", repr(trunc_e))

            _logger.info("TRACING: Exiting HamsHttpCase.tearDown")

    def browser_js(self, *args, **kwargs):
        _logger.info("TRACING: Entering browser_js wrapper.")
        _apply_cdp_hook(getattr(self, 'browser', None))
        try:
            super().browser_js(*args, **kwargs)
            _logger.info("TRACING: super().browser_js completed successfully.")
        except Exception as e: # audit-ignore-catch-all
            self.__class__._hams_tour_failed = True

            is_watchdog = False
            current_exc = e
            while current_exc is not None:
                if "socket is already closed" in str(current_exc) or "BrokenPipeError" in type(current_exc).__name__:
                    is_watchdog = True
                    break
                current_exc = getattr(current_exc, '__context__', None)

            if not is_watchdog and getattr(self, 'browser', None):
                try:
                    if hasattr(self.browser, 'take_screenshot'):
                        self.browser.take_screenshot()
                except Exception as ss_e: # audit-ignore-catch-all
                    _logger.warning("TRACING: Ignored Exception taking fallback screenshot: %s", repr(ss_e))

            if is_watchdog:
                raise AssertionError("Tour failed due to severed Chrome websocket.") from None
            else:
                raise e from None
        finally:
            _logger.info("TRACING: Exiting browser_js wrapper.")

    def start_tour(self, *args, **kwargs):
        _apply_cdp_hook(getattr(self, 'browser', None))
        args_list = list(args)

        tour_debug = os.environ.get("HAMS_TOUR_DEBUG")
        if tour_debug and args_list and isinstance(args_list[0], str):
            url_path = args_list[0]
            if "debug=" in url_path:
                url_path = re.sub(r'debug=[^&]*', f'debug={tour_debug}', url_path)
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
            self.__class__._hams_tour_failed = True
            _logger.error("\n=== TOUR FAILED OR HUNG. DUMPING COMPILED ASSETS ===")
            try:
                dump_path = '/var/tmp/failed_tour_bundle.js'

                prefix = f"/*\n{watchdog_shared.global_captured_stack}\n*/\n\n" if watchdog_shared.global_captured_stack else "/* No V8 CDP stack trace available (Thread did not hang; failed via standard JS Error or Assertion). */\n\n"

                with open(dump_path, 'w') as f:
                    f.write(prefix)
                _logger.error("Dumped compiled JS bundle to %s", dump_path)
            except Exception as inner_e:  # audit-ignore-catch-all
                _logger.warning("TRACING: Ignored Exception dumping bundle to /var/tmp: %s", repr(inner_e))

            if isinstance(e, AssertionError):
                raise e from None
            else:
                raise e from None
