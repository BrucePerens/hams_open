# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import logging
import hmac

from odoo import http, _
from odoo.http import request
from odoo.addons.distributed_redis_cache.redis_pool import redis, redis_pool

_logger = logging.getLogger(__name__)

MAX_FAILED_ATTEMPTS = 10
FAILED_ATTEMPTS_WINDOW = 60


class PagerDutyController(http.Controller):

    @http.route(
        "/api/v1/pager_duty/update_domains",
        type="jsonrpc",
        auth="public",
        methods=["POST"],
        csrf=False,
    )
    # [@ANCHOR: pager_duty:update_domains]
    def update_domains(self, domains=None, api_identity=None, **kwargs):
        """
        Receives a list of custom domains and updates the pager duty maintenance function.
        """
        if domains is None:
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_domain_api_i18n]  # fmt: skip
            return {"status": "error", "message": _("Empty payload")}

        # This endpoint is `auth="public"`, gated only by comparing
        # `api_identity` against a shared secret -- without a rate limit an
        # attacker gets unlimited guesses at that secret. Same Redis-counter
        # pattern as `pager.incident.report_incident`'s own rate limit
        # (see [@ANCHOR: pd_redis_rate_limit] in models/incident.py), but
        # counting only failed attempts per source IP rather than
        # debouncing every call, since legitimate callers may need to push
        # domain updates more than once within the window.
        remote_addr = request.httprequest.remote_addr or "unknown"
        redis_key = f"pager_duty_domain_api_failed_attempts:{remote_addr}"
        r_client = None
        if redis and redis_pool:
            try:
                r_client = redis.Redis(connection_pool=redis_pool)
                attempts = r_client.get(redis_key)
                if attempts and int(attempts) >= MAX_FAILED_ATTEMPTS:
                    _logger.warning(
                        "pager_duty update_domains: too many failed auth attempts from %s",
                        remote_addr,
                    )
                    # audit-ignore-i18n: Tested by [@ANCHOR: pd_domain_api_i18n]  # fmt: skip
                    return {"status": "error", "message": _("Too many attempts")}
            except (redis.exceptions.RedisError, Exception) as e:  # audit-ignore-catch-all
                _logger.warning("Redis rate limit check failed: %s", e)
                r_client = None

        stored_identity = request.env["zero_sudo.security.utils"]._get_system_param("pager_duty.domain_api_identity")
        if not stored_identity or not api_identity or not hmac.compare_digest(api_identity, stored_identity):
            if r_client:
                try:
                    pipe = r_client.pipeline()
                    pipe.incr(redis_key)
                    pipe.expire(redis_key, FAILED_ATTEMPTS_WINDOW)
                    pipe.execute()
                except (redis.exceptions.RedisError, Exception) as e:  # audit-ignore-catch-all
                    _logger.warning("Redis rate limit increment failed: %s", e)
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_domain_api_i18n]  # fmt: skip
            return {"status": "error", "message": _("Unauthorized")}

        # Call the model method to handle this using a service account
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        request.env["pager.check"].with_user(svc_uid).with_context(mail_notrack=True).update_lets_encrypt_domains(
            domains
        )
        return {"status": "success"}
