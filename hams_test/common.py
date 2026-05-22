# -*- coding: utf-8 -*-
import psutil
import logging
import time
import signal
import threading
from unittest.mock import MagicMock, patch
from odoo.tests.common import HttpCase, TransactionCase, ChromeBrowser

_logger = logging.getLogger(__name__)

class TourWatchdogError(Exception):
    pass

def _cross_process_abort_handler(signum, frame):
    _logger.error("TRACING: OS Signal %s received! Force-aborting hung thread.", signum)
    raise TourWatchdogError(f"FATAL: Cross-process abort triggered by signal {signum}. The test hung and was killed by the JS Watchdog Relay.")

# Register for SIGUSR1 to allow the external test runner to snipe hanging tests instantly
signal.signal(signal.SIGUSR1, _cross_process_abort_handler)

# =================================================================================
# SYSTEM OVERRIDE: Robust ChromeBrowser Lifecycle & Telemetry
# =================================================================================
_original_ws_req = ChromeBrowser._websocket_request
_original_chrome_stop = ChromeBrowser.stop
_original_take_screenshot = getattr(ChromeBrowser, 'take_screenshot', None)

def _robust_ws_req(self, method, params=None, timeout=10.0, **kwargs):
    process = getattr(self, '_process', None) or getattr(self, '_chrome_process', None)
    if process and process.poll() is not None:
        raise RuntimeError(f"FATAL: Chrome process (PID {process.pid}) died before CDP command '{method}'.")

    # Throttling to prevent Jules VM CPU starvation from tight polling loops
    if method == 'Runtime.evaluate':
        time.sleep(0.5)  # audit-ignore-sleep

    try:
        return _original_ws_req(self, method, params=params, timeout=timeout, **kwargs)
    except TimeoutError as e:
        if process and process.poll() is not None:
            raise RuntimeError(f"FATAL: Chrome process died during CDP command '{method}'.") from e
        raise e
    except Exception as e:  # audit-ignore-catch-all
        _logger.warning("WebSocket error during %s: %s", method, e)
        err_name = type(e).__name__
        if 'Connection' in err_name or 'Closed' in err_name or 'BrokenPipe' in err_name:
            raise RuntimeError(f"FATAL: WebSocket pipe broke during CDP command '{method}'. Error: {e}") from e
        raise e

def _robust_chrome_stop(self):
    process = getattr(self, '_process', None) or getattr(self, '_chrome_process', None)
    if process:
        try:
            pid = process.pid
            parent = psutil.Process(pid)
            # Recursively SIGKILL all child processes (renderers, network, GPU, crashpad)
            for child in parent.children(recursive=True):
                child.kill()
            parent.kill()
            _logger.info(f"Reaper V4: Eradicated Chrome process tree for PID {pid}.")
        except psutil.NoSuchProcess:
            pass
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Reaper V4: Could not terminate Chrome process tree: %s", e)

    try:
        _original_chrome_stop(self)
    except Exception as e:  # audit-ignore-catch-all
        _logger.info("Ignored exception during original ChromeBrowser.stop(): %s", e)

def _robust_take_screenshot(self, *args, **kwargs):
    if _original_take_screenshot:
        try:
            _original_take_screenshot(self, *args, **kwargs)
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Original screenshot failed: %s", e)

    try:
        telemetry_timeout = 5.0 * getattr(self, 'throttling_factor', 1.0)
        res = self._websocket_request('Runtime.evaluate', params={'returnByValue': True, 'expression': 'document.documentElement.outerHTML'}, timeout=telemetry_timeout)
        dom_html = res.get('result', {}).get('value', '<html><body>Failed to extract DOM state.</body></html>')
        dom_path = '/var/tmp/failed_tour_dom.html'
        with open(dom_path, 'w', encoding='utf-8') as f:
            f.write(dom_html)
        _logger.error(f"Dumped frozen DOM state to {dom_path}")
    except Exception as e: # audit-ignore-catch-all
        _logger.warning("Failed to dump DOM telemetry: %s", e)

ChromeBrowser._websocket_request = _robust_ws_req
ChromeBrowser.stop = _robust_chrome_stop
if _original_take_screenshot:
    ChromeBrowser.take_screenshot = _robust_take_screenshot


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
                f"DiagnosticMock Security Trip: Recursion depth limit ({self._max_depth}) exceeded "
                f"on mock '{self._mock_name or 'unnamed'}'. You likely have a cyclic patch or "
                f"are mocking a core Odoo registry propagation method."
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
    pass


class HamsHttpCase(HttpCase, SafePatchMixin):
    # [@ANCHOR: hams_http_case]

    def tearDown(self):
        # Apply OS-level timeout to teardown to prevent unkillable zombies
        original_alrm = signal.signal(signal.SIGALRM, _cross_process_abort_handler)
        signal.alarm(60) # 60 seconds hard cap for teardown
        try:
            super().tearDown()
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("TRACING: HamsHttpCase.tearDown caught exception: %s", e)
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, original_alrm)

    def start_tour(self, *args, **kwargs):
        try:
            super().start_tour(*args, **kwargs)
        except Exception as e:  # audit-ignore-catch-all
            _logger.error("\n=== TOUR FAILED OR HUNG. DUMPING COMPILED ASSETS ===")
            try:
                bundle = self.env['ir.qweb']._get_asset_bundle('web.assets_tests').js()
                dump_path = '/var/tmp/failed_tour_bundle.js'
                with open(dump_path, 'w') as f:
                    if isinstance(bundle, str):
                        f.write(bundle)
                    elif hasattr(bundle, 'decode'):
                        f.write(bundle.decode('utf-8'))
                    elif hasattr(bundle, 'raw'):
                        f.write(bundle.raw.decode('utf-8'))
                    else:
                        f.write(str(bundle))
                _logger.error(f"Dumped compiled JS bundle to {dump_path}")
            except Exception as inner_e:  # audit-ignore-catch-all
                _logger.error("Could not dump bundle to /var/tmp: %s", inner_e)

            raise e


class HamsIntegrationCase(HamsHttpCase):
    # [@ANCHOR: integration_daemon_testing]

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls._daemons = []

    @classmethod
    def tearDownClass(cls):
        env = cls.env
        daemon_utils = env["zero_sudo.daemon.utils"]
        for process in cls._daemons:
            daemon_utils.stop_daemon_process(process)
        cls._daemons.clear()
        super().tearDownClass()

    @classmethod
    def start_daemon(cls, script_path, args=None, env_vars=None, health_url=None, timeout=600):
        env = cls.env
        daemon_utils = env["zero_sudo.daemon.utils"]
        process = daemon_utils.start_daemon_process(script_path, args, env_vars)
        cls._daemons.append(process)

        if health_url:
            # Active Vitality Watchdog: Monitor OS process while awaiting HTTP health check
            ready_event = threading.Event()
            error_container = []

            def check_health():
                try:
                    daemon_utils.poll_health_check(health_url, timeout=timeout)
                    ready_event.set()
                except Exception as e: # audit-ignore-catch-all
                    _logger.error("Health check thread failed: %s", e)
                    error_container.append(e)

            t = threading.Thread(target=check_health, daemon=True)
            t.start()

            start_time = time.time()
            while t.is_alive():
                t.join(timeout=0.25)
                # Instantly abort if the process segfaults or exits during initialization
                if process.poll() is not None:
                    raise RuntimeError(f"FATAL: Daemon process '{script_path}' crashed with exit code {process.returncode} while waiting for health check!")
                if time.time() - start_time > timeout + 5:
                    raise TimeoutError(f"FATAL: Daemon health check for '{script_path}' timed out after {timeout} seconds.")

            if error_container:
                raise error_container[0]

        return process
