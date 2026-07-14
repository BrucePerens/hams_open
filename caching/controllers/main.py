# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import logging
from odoo import http, tools
from odoo.http import request
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection

_logger = logging.getLogger(__name__)


class ServiceWorkerController(http.Controller):

    @http.route("/sw.js", type="http", auth="public", sitemap=False, website=True)
    def service_worker(self):
        # [@ANCHOR: COMM_caching_sw_serve_route]
        # Verified by [@ANCHOR: COMM_test_service_worker_01]
        """
        Serves the Service Worker script from the root scope.
        Injects mtime (invalidation) and max file size (quota).
        """
        # Use Redis for JS content cache
        content = None
        try:
            r = get_redis_connection(request.env)
            cached_js = r.get("caching_sw_js_content")
            if cached_js:
                content = cached_js.decode('utf-8') if isinstance(cached_js, bytes) else cached_js
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Failed to retrieve SW JS from Redis: %s", e)

        if not content:
            try:
                with tools.file_open("caching/static/src/sw/sw.js", "r") as f:
                    content = f.read()
                try:
                    r = get_redis_connection(request.env)
                    r.setex("caching_sw_js_content", 86400, content)
                except Exception as e: # audit-ignore-catch-all
                    _logger.warning("Failed to cache SW JS in Redis: %s", e)
            except FileNotFoundError:
                raise request.not_found()

        # Multi-Website Awareness: Get params
        website = request.website or request.env['website'].get_current_website()
        if website:
            quota_mb = website.caching_safe_quota_mb
            in_v = website.caching_invalidation_version
        else:
            quota_mb = 35
            in_v = 1

        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid("caching.user_caching_service")

        if request.env.context.get("force_fs_scan"):
            request.env["caching.mixin"].with_user(svc_uid).force_invalidate_cache()

        latest_mtime, max_file_size = request.env["caching.mixin"].with_user(svc_uid).get_global_static_info(quota_mb)

        cache_name = f"odoo-assets-cache-{latest_mtime}-v{in_v}"
        content = content.replace("__CACHE_NAME__", cache_name)
        content = content.replace("__MAX_FILE_SIZE_BYTES__", str(max_file_size))
        content = content.replace("__MAX_STORAGE_BYTES__", str(quota_mb * 1024 * 1024))

        headers = [
            ("Content-Type", "application/javascript"),
            ("Cache-Control", "no-cache, max-age=0"),
        ]
        return request.make_response(content, headers=headers)
