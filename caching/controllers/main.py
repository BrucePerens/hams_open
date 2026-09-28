# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import logging
from odoo import http, tools
from odoo.http import request
import werkzeug.exceptions
from odoo.addons.distributed_redis_cache.redis_pool import redis, get_redis_connection

_logger = logging.getLogger(__name__)


class ServiceWorkerController(http.Controller):

    @http.route("/sw.js", type="http", auth="public", sitemap=False, website=True)
    def service_worker(self):
        # [@ANCHOR: COMM_caching_sw_serve_route]

        # # Verified by [@ANCHOR: COMM_test_service_worker_01]
        """
        Serves the Service Worker script from the root scope.
        Injects mtime (invalidation) and max file size (quota).
        """
        # Use Redis for JS content cache.
        #
        # Bug-hunt fix (2026-09-27, tier-1 pass): both Redis calls below used to
        # be unguarded. Redis here is a CACHE in front of a file on disk that is
        # the real source of truth -- every other get_redis_connection() call
        # site in this codebase (redis_cache.py's @distributed_cache read and
        # write, invalidate_model_cache, poll_and_clear_local_cache,
        # distributed_cache_config.check_redis_status) wraps its Redis call in
        # `except redis.RedisError` and degrades to the local/disk path. This
        # one did not, so an unreachable Redis turned the site's public,
        # unauthenticated, root-scope /sw.js into an HTTP 500 for every visitor
        # -- while the rest of the site kept serving normally, which is exactly
        # the inconsistency that makes it hard to diagnose. Degrade the same way
        # the siblings do: serve the template straight off disk, skip the cache
        # write, log it once per request at warning level.
        content = None
        r = get_redis_connection(request.env)
        try:
            cached_js = r.get("caching_sw_js_content")
        except redis.RedisError as e:
            _logger.warning("Redis unavailable serving /sw.js, reading from disk: %s", e)
            cached_js = None
            r = None
        if cached_js:
            content = cached_js.decode('utf-8') if isinstance(cached_js, bytes) else cached_js

        if not content:
            try:
                with tools.file_open("caching/static/src/sw/sw.js", "r") as f:
                    content = f.read()
            except FileNotFoundError:
                raise werkzeug.exceptions.NotFound()
            if r is not None:
                try:
                    r.setex("caching_sw_js_content", 86400, content)
                except redis.RedisError as e:
                    _logger.warning("Could not cache /sw.js template in Redis: %s", e)

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

        latest_mtime, max_file_size = request.env["caching.mixin"].get_global_static_info(quota_mb, override_svc_uid=svc_uid)

        cache_name = f"odoo-assets-cache-{latest_mtime}-v{in_v}"
        content = content.replace("__CACHE_NAME__", cache_name)
        content = content.replace("__MAX_FILE_SIZE_BYTES__", str(max_file_size))
        content = content.replace("__MAX_STORAGE_BYTES__", str(quota_mb * 1024 * 1024))

        # A real, deliberate per-deployment switch (see security_utils.py's
        # own comment on caching.enable_sw_test_hooks) -- not derived from
        # Odoo's own test_enable, on purpose. Off (test hooks compiled out
        # of the served script) unless a human has explicitly turned this
        # on for this specific Odoo instance.
        test_hooks_enabled = request.env["zero_sudo.security.utils"]._get_system_param(
            "caching.enable_sw_test_hooks", "False"
        )
        content = content.replace(
            "__TEST_HOOKS_ENABLED__", "true" if str(test_hooks_enabled).lower() in ("1", "true") else "false"
        )

        headers = [
            ("Content-Type", "application/javascript"),
            ("Cache-Control", "no-cache, max-age=0"),
        ]
        return request.make_response(content, headers=headers)

# # Verified by [@ANCHOR: COMM_test_service_worker_01]

# # Verified by [@ANCHOR: COMM_test_caching_sudo_params]
