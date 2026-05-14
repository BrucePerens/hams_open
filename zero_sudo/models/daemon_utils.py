# -*- coding: utf-8 -*-
import logging
import os
import signal
import subprocess
import time
import urllib.request
from odoo import models, api, _
from odoo.exceptions import UserError

_logger = logging.getLogger(__name__)

class ZeroSudoDaemonUtils(models.AbstractModel):
    _name = "zero_sudo.daemon.utils"
    _description = "Daemon Management Utilities"

    @api.model
    def start_daemon_process(self, script_path, args=None, env_vars=None):
        """Starts a python daemon script as a subprocess."""
        cmd = ['python3', script_path] + (args or [])
        env = os.environ.copy()
        if env_vars:
            env.update(env_vars)

        _logger.info("Starting daemon: %s", " ".join(cmd))
        process = subprocess.Popen(cmd, env=env)
        return process

    @api.model
    def stop_daemon_process(self, process):
        """Safely terminates a daemon process."""
        if process and process.poll() is None:
            _logger.info("Stopping daemon PID %s", process.pid)
            try:
                os.kill(process.pid, signal.SIGTERM)
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                _logger.warning("Daemon PID %s did not terminate, forcing SIGKILL", process.pid)
                os.kill(process.pid, signal.SIGKILL)

    @api.model
    def poll_health_check(self, url, timeout=30, interval=1):
        """Polls a URL until it returns 200 OK or times out."""
        _logger.info("Polling health check %s for up to %s seconds", url, timeout)
        start_time = time.time()
        while time.time() - start_time < timeout:
            try:
                req = urllib.request.Request(url, method="HEAD")
                with urllib.request.urlopen(req, timeout=interval) as response:
                    if response.status == 200:
                        _logger.info("Health check %s passed.", url)
                        return True
            except Exception:
                pass
            time.sleep(interval)  # audit-ignore-sleep

        error_msg = _("Daemon health check failed for %s after %s seconds.") % (url, timeout)
        _logger.error(error_msg)
        raise UserError(error_msg)
