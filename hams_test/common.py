# -*- coding: utf-8 -*-
import logging
import signal
import time
import urllib.request
import threading
from unittest.mock import MagicMock, patch
from odoo.tests.common import HttpCase, TransactionCase

_logger = logging.getLogger(__name__)

class TourWatchdogError(Exception):
    pass

def _timeout_handler(signum, frame):
    signal.signal(signum, signal.SIG_IGN) # Debounce
    _logger.error("TRACING: OS Signal %s (Timeout) received! Force-aborting hung thread.", signum)
    raise TourWatchdogError(f"Test step timed out and was aborted by OS signal {signum}.")

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

    @classmethod
    def setUpClass(cls):
        # 🚨 THE ANTI-HANG INJECTION 🚨
        # Odoo's native HttpCase spawns non-daemon threads for Werkzeug and Chrome websockets.
        # If these connections deadlock, the non-daemon threads prevent Python from exiting,
        # causing the entire test runner to hang infinitely.
        # We intercept thread.start() to force EVERY thread spawned during setup to be a daemon.
        original_start = threading.Thread.start

        def _daemonized_start(self_thread, *args, **kwargs):
            self_thread.daemon = True
            return original_start(self_thread, *args, **kwargs)

        with patch.object(threading.Thread, 'start', new=_daemonized_start):
            super().setUpClass()

    @classmethod
    def tearDownClass(cls):
        _logger.info("TRACING: Entering HamsHttpCase.tearDownClass.")

        # 1. Aggressively murder Chrome first to sever all open websockets
        if hasattr(cls, 'browser') and cls.browser:
            if hasattr(cls.browser, 'chrome_process'):
                try:
                    cls.browser.chrome_process.kill()
                    _logger.info("TRACING: Aggressively killed chrome_process to drop websockets.")
                except OSError as e:
                    _logger.error("TRACING: Ignored OSError killing chrome process: %s", e)

            # Neutralize browser websocket join
            if hasattr(cls.browser, '_websocket_thread') and cls.browser._websocket_thread:
                cls.browser._websocket_thread.join = lambda *args, **kwargs: None

        # 2. Defuse Server Thread Join
        if hasattr(cls, 'server_thread') and cls.server_thread:
            cls.server_thread.join = lambda *args, **kwargs: None

        # 3. Defuse Server Stop
        if hasattr(cls, 'server') and cls.server:
            try:
                if hasattr(cls.server, 'server') and hasattr(cls.server.server, 'socket'):
                    cls.server.server.socket.close()
            except OSError as e:
                _logger.error("TRACING: Ignored OSError closing server socket: %s", e)
            except Exception as e: # audit-ignore-catch-all
                _logger.exception("TRACING: Ignored Exception closing server socket: %s", e)
            cls.server.stop = lambda *args, **kwargs: None

        # 4. OS Timeout for super().tearDownClass()
        original_alrm = signal.signal(signal.SIGALRM, _timeout_handler)
        signal.alarm(30)
        try:
            super().tearDownClass()
            _logger.info("TRACING: Successfully completed super().tearDownClass()")
        except Exception as e: # audit-ignore-catch-all
            _logger.exception("TRACING: Native teardown failed or hung: %s", e)
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, original_alrm)
            _logger.info("TRACING: Exiting HamsHttpCase.tearDownClass")

    def tearDown(self):
        # Apply OS-level timeout to teardown to prevent unkillable zombies
        _logger.info("TRACING: Entering HamsHttpCase.tearDown")
        original_alrm = signal.signal(signal.SIGALRM, _timeout_handler)
        signal.alarm(60) # 60 seconds hard cap for teardown
        try:
            super().tearDown()
            _logger.info("TRACING: Completed super().tearDown")
        except Exception as e: # audit-ignore-catch-all
            _logger.exception("TRACING: HamsHttpCase.tearDown caught exception: %s", e)
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, original_alrm)
            _logger.info("TRACING: Exiting HamsHttpCase.tearDown")

    def browser_js(self, *args, **kwargs):
        _logger.info("TRACING: Entering browser_js wrapper. Setting 60s SIGALRM.")
        original_alrm = signal.signal(signal.SIGALRM, _timeout_handler)
        signal.alarm(60)
        try:
            super().browser_js(*args, **kwargs)
            _logger.info("TRACING: super().browser_js completed successfully.")
        except Exception as e: # audit-ignore-catch-all
            _logger.exception("TRACING: super().browser_js failed, flagging tour as failed to prevent shutdown hang: %s", e)
            self.__class__._hams_tour_failed = True
            raise
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, original_alrm)
            _logger.info("TRACING: Exiting browser_js wrapper.")

    def start_tour(self, *args, **kwargs):
        _logger.info("TRACING: Entering start_tour wrapper.")
        try:
            super().start_tour(*args, **kwargs)
            _logger.info("TRACING: super().start_tour completed successfully.")
        except Exception as e:  # audit-ignore-catch-all
            _logger.exception("TRACING: Exception caught in start_tour wrapper: %s", e)
            self.__class__._hams_tour_failed = True
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
                _logger.error("Dumped compiled JS bundle to %s", dump_path)
            except Exception as inner_e:  # audit-ignore-catch-all
                _logger.exception("Could not dump bundle to /var/tmp: %s", inner_e)

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
            start_time = time.time()
            is_healthy = False
            while time.time() - start_time < timeout:
                if process.poll() is not None:
                    raise RuntimeError(f"FATAL: Daemon process '{script_path}' crashed with exit code {process.returncode} while waiting for health check!")
                try:
                    req = urllib.request.Request(health_url)
                    with urllib.request.urlopen(req, timeout=1.0) as response:
                        if response.getcode() in (200, 204):
                            is_healthy = True
                            break
                except Exception as e: # audit-ignore-catch-all
                    _logger.exception("Daemon health check not ready yet: %s", e)
                time.sleep(0.5) # audit-ignore-sleep

            if not is_healthy:
                raise TimeoutError(f"FATAL: Daemon health check for '{script_path}' timed out after {timeout} seconds.")

        return process
