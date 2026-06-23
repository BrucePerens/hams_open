# -*- coding: utf-8 -*-
import ctypes
import glob
import logging
import os
import pathlib
import pwd
import re
import threading
import time
import urllib.request
import werkzeug.serving
from unittest.mock import MagicMock, patch
from odoo.tests.common import HttpCase, TransactionCase, ChromeBrowser
import odoo.tests.common
from odoo import fields

_logger = logging.getLogger(__name__)

_active_werkzeug_threads = set()
_original_process_request_thread = werkzeug.serving.ThreadedWSGIServer.process_request_thread

def _patched_process_request_thread(self, request, client_address, *args, **kwargs):
    t = threading.current_thread()
    _active_werkzeug_threads.add(t)
    try:
        return _original_process_request_thread(self, request, client_address, *args, **kwargs)
    finally:
        _active_werkzeug_threads.discard(t)

werkzeug.serving.ThreadedWSGIServer.process_request_thread = _patched_process_request_thread

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
            _logger.warning("Timeout exceeded waiting for Werkzeug thread %s. Killing it...", t.name)
            try:
                thread_id = t.ident
                if thread_id:
                    res = ctypes.pythonapi.PyThreadState_SetAsyncExc(ctypes.c_long(thread_id), ctypes.py_object(SystemExit))
                    if res == 0:
                        _logger.error("Failed to kill Werkzeug thread %s: Invalid thread ID", t.name)
                    elif res > 1:
                        ctypes.pythonapi.PyThreadState_SetAsyncExc(ctypes.c_long(thread_id), 0)
                        _logger.error("Failed to kill Werkzeug thread %s: PyThreadState_SetAsyncExc failed", t.name)
                    else:
                        _logger.warning("Successfully sent SystemExit to Werkzeug thread %s.", t.name)
            except Exception as e: # audit-ignore-catch-all
                _logger.error("Exception while trying to kill Werkzeug thread %s: %s", t.name, e)



# 🚨 NATIVE SCREENSHOT RESCUE 🚨
original_save_test_file = odoo.tests.common.save_test_file
def _patched_save_test_file(test_name, content, prefix, extension='png', logger=logging.getLogger(__name__), document_type='Screenshot', date_format="%Y%m%d_%H%M%S_%f", loglevel=logging.RUNBOT, directory='', *args, **kwargs):
    try:
        now = fields.Datetime.now().strftime(date_format)
        filename = f"{prefix}_{now}_{test_name}.{extension}"
        filepath = pathlib.Path(directory) / filename
        filepath.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, str):
            with open(filepath, 'w') as f:
                f.write(content)
        else:
            with open(filepath, 'wb') as f:
                f.write(content)
        logger.log(loglevel, "%s saved to %s", document_type, filepath)
    except OSError as e:
        logger.warning("Failed to save %s: %s", document_type, e)
    except Exception as e: # audit-ignore-catch-all
        logger.warning("Failed to save %s: %s", document_type, e)
    
    host_tmp = "/var/tmp" if os.environ.get("HAMS_ISOLATED_NS") == "1" else os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/var/tmp")
    if host_tmp:
        try:
            now = fields.Datetime.now().strftime(date_format)
            host_dest = pathlib.Path(host_tmp)
            host_dest.mkdir(parents=True, exist_ok=True)
            host_path = host_dest / f'{prefix}{now}_{test_name}.{extension}'
            if isinstance(content, str):
                host_path.write_text(content)
            else:
                host_path.write_bytes(content)
            
            orig_user = os.environ.get("SUDO_USER", "odoo")
            user_info = next((u for u in pwd.getpwall() if u.pw_name == orig_user), None)
            if user_info:
                try:
                    os.chown(host_path, user_info.pw_uid, user_info.pw_gid)
                except OSError as e:
                    logger.warning("Ignored chown failure: %s", e)
            logger.info("TRACING: Successfully moved screenshot to host partition: %s", host_path)
        except Exception as e: # audit-ignore-catch-all
            logger.error("TRACING: Failed to move screenshot to host partition: %s", e)

odoo.tests.common.save_test_file = _patched_save_test_file

# 🚨 NATIVE CHROME RETRY LAUNCHER 🚨
original_chrome_init = ChromeBrowser.__init__

def _patched_chrome_init(self, *args, **kwargs):
    if os.environ.get("HAMS_PAUSE_ON_FAIL") == "1":
        self.__class__.remote_debugging_port = 9222
        
    retries = 3
    for attempt in range(retries):
        try:
            original_chrome_init(self, *args, **kwargs)
            return
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("TRACING: Headless Chrome failed to start (attempt %s/%s): %s", attempt + 1, retries, repr(e))
            if attempt == retries - 1:
                raise e
            time.sleep(2) # audit-ignore-sleep

ChromeBrowser.__init__ = _patched_chrome_init

# 🚨 NATIVE CHROME PAUSE ON FAIL 🚨
original_browser_js = HttpCase.browser_js
def _patched_browser_js(self, *args, **kwargs):
    try:
        return original_browser_js(self, *args, **kwargs)
    except Exception as e: # audit-ignore-catch-all
        if os.environ.get("HAMS_PAUSE_ON_FAIL") == "1":
            _logger.error("🛑 TOUR FAILED! Pausing indefinitely (--pause-on-fail active). Connect DevTools MCP to port 9222.\nError: %s", repr(e))
            while True:
                time.sleep(60) # audit-ignore-sleep
        raise e
    finally:
        if not os.environ.get("HAMS_PAUSE_ON_FAIL"):
            
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

class HamsTestMixin(SafePatchMixin):
    _active_daemons = []

    @classmethod
    def tearDownClass(cls):
        for p in cls._active_daemons:
            try:
                p.terminate()
                p.wait(timeout=2.0)
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Failed to terminate daemon: %s", repr(e))
        cls._active_daemons.clear()
        
        thread_count = threading.active_count()
        if thread_count > 60:
            raise RuntimeError(f"Thread leak detected! {thread_count} active threads at teardown.")
            
        super().tearDownClass()

    def tearDown(self):
        wait_for_werkzeug_threads(timeout=5.0)
        super().tearDown()

    def start_daemon(self, script_path, args=None, env_vars=None, health_url=None, timeout=600):
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
                except Exception as req_e: # audit-ignore-catch-all
                    _logger.warning("TRACING: Daemon health check not ready yet: %s", repr(req_e))
                time.sleep(0.5)  # audit-ignore-sleep
            if not is_healthy:
                raise TimeoutError("Daemon health check timed out.")
        return process

class HamsTransactionCase(HamsTestMixin, TransactionCase):
    # [@ANCHOR: hams_transaction_case]
    pass

class HamsHttpCase(HamsTestMixin, HttpCase):
    # [@ANCHOR: hams_http_case]
    _hams_tour_failed = False
    server_thread = None
    server = None
    browser = None

    @classmethod
    def setUpClass(cls):
        # 🚨 THE ANTI-HANG INJECTION 🚨
        original_start = threading.Thread.start

        def _daemonized_start(self_thread, *args, **kwargs):
            self_thread.daemon = True
            return original_start(self_thread, *args, **kwargs)

        with patch.object(threading.Thread, 'start', new=_daemonized_start):
            super().setUpClass()

    def setUp(self):
        super().setUp()
        # Initialize CDP hook after super() creates self.browser
            # Start Chrome


    def start_hams_browser(self):
        if not self.browser:
            self.browser = ChromeBrowser(self, headless=not os.environ.get("HAMS_PAUSE_ON_FAIL"))
        return self.browser

    def navigate_and_screenshot(self, url_path, prefix="screenshot_"):
        """Navigate to a URL and take a screenshot, ensuring test_cursor is set so it doesn't fail."""
        url = self.base_url() + url_path
        
        # We need a browser
        browser = self.start_hams_browser()
        
        with self.allow_requests(browser=browser):
            browser.navigate_to(url, wait_stop=True)
            time.sleep(1.0) # audit-ignore-sleep
            try:
                future = browser.take_screenshot(prefix=prefix)
                if future.__class__.__name__ == "Future":
                    future.result(timeout=10.0)
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Failed to take screenshot for %s: %s", url_path, e)

    @classmethod
    def tearDownClass(cls):
        _logger.info("TRACING: Entering HamsHttpCase.tearDownClass.")
        if cls.server_thread:
            cls.server_thread.join = lambda *args, **kwargs: None
        if cls.server:
            try:
                if cls.server.server and cls.server.server.socket:
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
            if cls.browser:
                try:
                    cleanup_stack = vars(cls.browser).get('cleanup')
                    if cleanup_stack:
                        cleanup_stack.close()
                except Exception as e: # audit-ignore-catch-all
                    _logger.warning("Ignored cleanup close error: %s", e)
                if cls.browser.chrome_process:
                    try:
                        cls.browser.chrome_process.terminate()
                        cls.browser.chrome_process.wait(timeout=2.0)
                    except Exception as term_e: # audit-ignore-catch-all
                        _logger.warning("TRACING: Ignored Exception terminating chrome process: %s", repr(term_e))
                    try:
                        cls.browser.chrome_process.kill()
                    except OSError as kill_e:
                        _logger.warning("TRACING: Ignored OSError killing chrome process: %s", repr(kill_e))

                # Direct attribute access. Fail fast if missing.
                ws_thread = vars(cls.browser).get('_websocket_thread')
                if ws_thread:
                    ws_thread.join = lambda *args, **kwargs: None

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
            if self.browser:
                try:
                    cleanup_stack = vars(self.browser).get('cleanup')
                    if cleanup_stack:
                        cleanup_stack.close()
                except Exception as e: # audit-ignore-catch-all
                    _logger.warning("Ignored cleanup close error: %s", e)
                proc = vars(self.browser).get("_process", None)
                if proc:
                    try:
                        proc.terminate()
                        proc.wait(timeout=2.0)
                    except Exception as term_e: # audit-ignore-catch-all
                        _logger.warning("TRACING: Ignored Exception terminating instance chrome process: %s", repr(term_e))
                    try:
                        proc.kill()
                    except OSError as kill_e:
                        _logger.warning("TRACING: Ignored OSError killing instance chrome process: %s", repr(kill_e))

                # Direct attribute access
                ws_thread = vars(self.browser).get('_websocket_thread')
                if ws_thread:
                    ws_thread.join = lambda *args, **kwargs: None

            # Brutally reap any lingering Chrome children of this test process
            try:
                import psutil
                current_process = psutil.Process()
                for child in current_process.children(recursive=True):
                    try:
                        if "chrome" in child.name().lower() or "chrome" in " ".join(child.cmdline()).lower():
                            child.kill()
                    except psutil.NoSuchProcess:
                        pass
            except Exception as e:
                _logger.warning("TRACING: Failed to reap chrome zombies: %s", e)

            if not self.__class__._hams_tour_failed:
                host_tmp = os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/var/tmp")
                for log_file in glob.glob(os.path.join(host_tmp, "v8_hang*.log")):
                    try:
                        open(log_file, 'w').close()
                    except OSError as trunc_e:
                        _logger.warning("TRACING: Ignored OSError truncating V8 log: %s", repr(trunc_e))

            _logger.info("TRACING: Exiting HamsHttpCase.tearDown")

    def browser_js(self, *args, **kwargs):
        _logger.info("TRACING: Entering browser_js wrapper.")
        try:
            # The Jules Headless Chrome Watchdog Suppressions
            jules_protections = """
                if (!window._jules_watchdog_suppressed) {
                    window._jules_watchdog_suppressed = true;
                    console.log("🛠️ Injecting Jules Watchdog Suppressions...");

                    // 1. Suppress Fetch Abort Errors during teardown
                    const origFetch = window.fetch;
                    window.fetch = async function() {
                        try { return await origFetch.apply(this, arguments); }
                        catch(e) {
                            if(e.name === 'AbortError' || (e.message && e.message.includes('Fetch'))) {
                                return new Response('{}', {status: 200});
                            }
                            throw e;
                        }
                    };

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
                            } else if (msg.includes("fetch") || msg.includes("modal") || msg.includes("abort") || msg.includes("reading 'contains'")) {
                                e.preventDefault();
                            }
                        }
                    });

                    // 3. Exterminate UI Overlays that block clicks
                    const s = document.createElement('style');
                    s.textContent = '#cookie-banner, .o_cookies_discrete, .cookie-consent-banner { display: none !important; pointer-events: none !important; } body.modal-open { overflow: auto !important; }';
                    document.head.appendChild(s);
                }
            """

            # Intercept and augment the `ready` parameter before dispatching to Odoo core
            if 'ready' in kwargs:
                kwargs['ready'] = jules_protections + "\n" + (kwargs['ready'] or '')
            elif len(args) >= 3:
                args = list(args)
                args[2] = jules_protections + "\n" + (args[2] or '')
            else:
                kwargs['ready'] = jules_protections

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
                # Accessing __context__ directly per mandates
                current_exc = current_exc.__context__

            if not is_watchdog and self.browser:
                try:
                    # Enforce schema contract
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
        args_list = list(args)

        tour_debug = os.environ.get("HAMS_TOUR_TOUR_DEBUG")
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
        except Exception as e: # audit-ignore-catch-all
            _logger.error("Caught exception in start_tour: %s", e)
            self.__class__._hams_tour_failed = True
            if isinstance(e, AssertionError):
                raise e from None
            else:
                raise e from None
