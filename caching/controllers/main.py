# -*- coding: utf-8 -*-
import os
import logging
import threading
from odoo import http, tools
from odoo.http import request
from odoo.modules.module import get_module_path

_logger = logging.getLogger(__name__)

class ServiceWorkerController(http.Controller):

    _fs_cache = None
    _fs_lock = threading.Lock()

    def _get_fs_stats(self):
        # [@ANCHOR: caching_fs_scan_logic]
        # Verified by [@ANCHOR: test_settings_and_cache_01]
        """
        Scans the 'static/' directories of all installed modules.
        Returns a tuple: (latest_mtime, file_sizes).
        Cached in RAM at the class level so the disk walk only executes once per worker lifecycle.
        """
        if type(self)._fs_cache:
            return type(self)._fs_cache

        with type(self)._fs_lock:
            if type(self)._fs_cache:
                return type(self)._fs_cache

            max_mtime = 0.0
            file_sizes = []

            # Raw SQL to get installed modules quickly without ORM/sudo overhead.
            # This only retrieves module names that are already public knowledge
            # and is required to calculate the SW footprint before it is served.
            request.env.cr.execute(
                "SELECT name FROM ir_module_module WHERE state = 'installed'"
            )
            installed_modules = [row[0] for row in request.env.cr.fetchall()]

            for module_name in installed_modules:
                mod_path = get_module_path(module_name)
                if not mod_path:
                    continue

                static_path = os.path.join(mod_path, "static")
                if os.path.exists(static_path):
                    for root, dirs, files in os.walk(static_path):
                        for file in files:
                            filepath = os.path.join(root, file)
                            try:
                                mtime = os.path.getmtime(filepath)
                                if mtime > max_mtime:
                                    max_mtime = mtime
                                file_sizes.append(os.path.getsize(filepath))
                            except OSError as e:
                                _logger.warning("Could not access file %s: %s", filepath, e)

            file_sizes.sort(reverse=True)
            res = (max_mtime, file_sizes)
            type(self)._fs_cache = res
            return res

    def _get_global_static_info(self):
        # [@ANCHOR: caching_quota_calculation]
        # Verified by [@ANCHOR: test_settings_and_cache_01]
        """
        Returns a tuple: (latest_mtime_string, dynamic_max_file_size_string).
        Calculates the safe dynamic max file size based on configurable quota.
        """
        max_mtime, file_sizes = self._get_fs_stats()

        #
        quota_mb = int(request.env['zero_sudo.security.utils']._get_system_param('caching.safe_quota_mb', '35') or 35) # Tested by [@ANCHOR: test_caching_sudo_params]

        # Reserve 15MB for Odoo's compiled /web/assets/ bundles and overhead
        SAFE_QUOTA = quota_mb * 1024 * 1024

        total_size = sum(file_sizes)

        if not file_sizes:
            return (str(int(max_mtime)), str(10 * 1024 * 1024))  # Default 10MB

        if total_size <= SAFE_QUOTA:
            # Everything fits. Set max size to just above the largest file.
            dynamic_max_size = file_sizes[0] + 1024 if file_sizes else 10 * 1024 * 1024
        else:
            # We need to drop the largest files until the remaining sum fits in the quota
            current_total = total_size
            dynamic_max_size = 0
            for size in file_sizes:
                current_total -= size
                if current_total <= SAFE_QUOTA:
                    # The file we just dropped ('size') must be rejected by the SW.
                    # Set the limit to 1 byte less than that file's size.
                    dynamic_max_size = size - 1
                    break

        return (str(int(max_mtime)), str(dynamic_max_size))

    @http.route("/sw.js", type="http", auth="public", sitemap=False)
    def service_worker(self):
        # [@ANCHOR: caching_sw_serve_route]
        # Verified by [@ANCHOR: test_service_worker_01]
        """
        Serves the Service Worker script from the root scope.
        Dynamically injects the latest filesystem mtime (for cache invalidation)
        and the calculated max file size (for quota protection).
        """
        try:
            with tools.file_open("caching/static/src/sw/sw.js", "r") as f:
                content = f.read()
        except FileNotFoundError:
            return request.not_found()

        latest_mtime, max_file_size = self._get_global_static_info()

        invalidation_version = request.env['zero_sudo.security.utils']._get_system_param('caching.invalidation_version', '1') # Tested by [@ANCHOR: test_caching_sudo_params]

        content = content.replace("__CACHE_NAME__", f"odoo-assets-cache-{latest_mtime}-v{invalidation_version}")
        content = content.replace("__MAX_FILE_SIZE_BYTES__", max_file_size)

        headers = [
            ("Content-Type", "application/javascript"),
            ("Cache-Control", "no-cache, max-age=0"),
        ]
        return request.make_response(content, headers=headers)
