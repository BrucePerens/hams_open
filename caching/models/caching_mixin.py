# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import os
import logging
from odoo import models, api, fields
from odoo.modules.module import get_module_path
import odoo.modules.module

from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, invalidate_model_cache

_logger = logging.getLogger(__name__)


class CachingMixin(models.AbstractModel):
    _name = 'caching.mixin'
    _description = 'Caching Utilities'
    name = fields.Char(string="Name", default="Caching Mixin")

    @api.model
    @distributed_cache()
    def get_fs_stats(self):
        # [@ANCHOR: caching_fs_scan_logic]
        # Verified by [@ANCHOR: test_settings_and_cache_01]
        # Verified by [@ANCHOR: test_caching_zero_sudo_scan]
        """
        Scans 'static/' dirs of all installed modules.
        Returns tuple: (latest_mtime, file_sizes).
        """
        max_mtime = 0.0
        file_sizes = []

        # Get installed modules from in-memory registry
        installed_modules = odoo.modules.module.get_modules()

        def _scan_recursive(path):
            nonlocal max_mtime
            try:
                with os.scandir(path) as it:
                    for entry in it:
                        if entry.name.startswith("."):
                            continue
                        if entry.is_dir(follow_symlinks=False):
                            _scan_recursive(entry.path)
                        elif entry.is_file(follow_symlinks=False):
                            stat = entry.stat()
                            if stat.st_mtime > max_mtime:
                                max_mtime = stat.st_mtime
                            file_sizes.append(stat.st_size)
            except OSError as e:
                _logger.warning("Could not access path %s: %s", path, e)

        for module_name in installed_modules:
            mod_path = get_module_path(module_name)
            if not mod_path:
                continue

            static_path = os.path.join(mod_path, "static")
            if os.path.isdir(static_path):
                _scan_recursive(static_path)

        file_sizes.sort(reverse=True)
        return (max_mtime, file_sizes)

    @api.model
    @distributed_cache()
    def get_global_static_info(self, quota_mb):
        # [@ANCHOR: caching_quota_calculation]
        # Verified by [@ANCHOR: test_settings_and_cache_01]
        """
        Calculates the safe dynamic max file size based on quota.
        Returns tuple: (latest_mtime_string, dynamic_max_size_string).
        """
        max_mtime, file_sizes = self.get_fs_stats()

        SAFE_QUOTA = max(0, quota_mb - 10) * 1024 * 1024
        total_size = sum(file_sizes)

        if SAFE_QUOTA <= 0:
            return (str(int(max_mtime)), "0")

        if not file_sizes:
            return (str(int(max_mtime)), str(10 * 1024 * 1024))

        if total_size <= SAFE_QUOTA:
            dynamic_max_size = max(
                file_sizes[0] + 1024 if file_sizes else 0,
                10 * 1024 * 1024 if SAFE_QUOTA > 0 else 0,
            )
        else:
            current_total = total_size
            dynamic_max_size = 0
            for size in file_sizes:
                current_total -= size
                if current_total <= SAFE_QUOTA:
                    dynamic_max_size = size - 1
                    break

        return (str(int(max_mtime)), str(dynamic_max_size))

    @api.model
    def force_invalidate_cache(self):
        invalidate_model_cache(self.env, self._name)
