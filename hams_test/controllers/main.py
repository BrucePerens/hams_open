import subprocess
import logging
from odoo import http

_logger = logging.getLogger(__name__)

class HamsTestWatchdog(http.Controller):
    @http.route('/hams_test/watchdog/kill', type='jsonrpc', auth='none')
    def kill_hung_browser(self, diagnostic=None, **kwargs):
        _logger.error("🚨 [WATCHDOG] Shared Worker triggered kill! 🚨")
        if diagnostic:
            _logger.error("Diagnostic Dump:\n%s", diagnostic)

        _logger.error("Forcefully terminating headless Chrome to break the V8 loop...")
        subprocess.run(["pkill", "-9", "-f", "chrome.*--headless"], check=False)
        return {"status": "killed"}
