import subprocess
import logging
import os
import re
import time
from odoo import http

_logger = logging.getLogger(__name__)

class HamsTestWatchdog(http.Controller):
    @http.route('/hams_test/watchdog/dump', type='jsonrpc', auth='none')
    def dump_diagnostic(self, diagnostic=None, log=None, **kwargs):
        _logger.error("🚨 [WATCHDOG] Shared Worker dumping V8 loop diagnostic! 🚨")

        tour_name = "unknown_tour"
        if log:
            match = re.search(r'([a-zA-Z0-9_]+_tour)', log)
            if match:
                tour_name = match.group(1)

        # Sanitize to prevent path traversal
        safe_name = re.sub(r'[^a-zA-Z0-9_]', '', tour_name)
        filename = os.path.join("/var/tmp", f"v8_hang_{safe_name}_{int(time.time())}.log")

        try:
            with open(filename, "w") as f:  # audit-ignore-path-traversal
                f.write(diagnostic or "No diagnostic provided.")
            _logger.error("Successfully wrote V8 hang diagnostic to %s", filename)
        except Exception as e:  # audit-ignore-catch-all
            _logger.error("Failed to write diagnostic to %s: %s", filename, e)

        return {"status": "dumped", "filename": filename}

    @http.route('/hams_test/watchdog/kill', type='jsonrpc', auth='none')
    def kill_hung_browser(self, **kwargs):
        _logger.error("🚨 [WATCHDOG] Shared Worker requested Chrome termination! 🚨")
        _logger.error("Forcefully terminating headless Chrome to break the V8 loop...")
        subprocess.run(["pkill", "-9", "-f", "chrome.*--headless"], check=False)
        return {"status": "killed"}
