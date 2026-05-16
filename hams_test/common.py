# -*- coding: utf-8 -*-
import logging
from odoo.tests.common import HttpCase

_logger = logging.getLogger(__name__)

class HamsIntegrationCase(HttpCase):
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
        daemon_utils = env['zero_sudo.daemon.utils']
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
        daemon_utils = env['zero_sudo.daemon.utils']
        process = daemon_utils.start_daemon_process(script_path, args, env_vars)
        cls._daemons.append(process)

        if health_url:
            daemon_utils.poll_health_check(health_url, timeout=timeout)
        return process
