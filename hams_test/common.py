# -*- coding: utf-8 -*-
import logging
from unittest.mock import MagicMock, patch
from odoo.tests.common import HttpCase, TransactionCase

_logger = logging.getLogger(__name__)


class DiagnosticMock(MagicMock):
    """
    A strict mock designed to trap runaway recursion and excessive deep calling
    often seen when Odoo registry models are incorrectly shadowed or cyclically patched.
    """
    def __init__(self, *args, **kwargs):
        # ADR-0012: Prevent runaway test execution by hard-capping mock recursion.
        max_depth = kwargs.pop("max_recursion_depth", 5)
        # MUST call super before setting attributes to avoid Py3.13 MagicMock getattr crash
        super().__init__(*args, **kwargs)
        self._max_depth = max_depth
        self._current_depth = 0

    def __call__(self, *args, **kwargs):
        self._current_depth += 1
        if self._current_depth > self._max_depth:
            self._current_depth = 0  # Reset for subsequent isolated assertions
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
    """
    Mixin to provide safe, runtime-only patching to avoid Odoo registry
    early-import corruption and mock recursion traps.
    """
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
    """
    Base class for standard transaction tests enforcing safe patching.
    """
    pass


class HamsHttpCase(HttpCase, SafePatchMixin):
    # [@ANCHOR: hams_http_case]
    """
    Base class for standard HTTP/UI Tour tests enforcing safe patching.
    """
    pass


class HamsIntegrationCase(HamsHttpCase):
    # [@ANCHOR: integration_daemon_testing]
    """
    Base class for heavy I/O integration tests.
    Automatically starts and stops required external daemons.
    """

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
    def start_daemon(cls, script_path, args=None, env_vars=None, health_url=None, timeout=30):
        """
        Starts a daemon and waits for it to become healthy.
        Must be called within setUpClass or setUp.
        """
        env = cls.env
        daemon_utils = env["zero_sudo.daemon.utils"]
        process = daemon_utils.start_daemon_process(script_path, args, env_vars)
        cls._daemons.append(process)

        if health_url:
            daemon_utils.poll_health_check(health_url, timeout=timeout)
        return process
