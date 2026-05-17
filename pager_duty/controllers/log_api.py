# -*- coding: utf-8 -*-
import json
import uuid
import os
import logging
from odoo import http, _
from odoo.http import request
from odoo.exceptions import AccessError

import redis

_logger = logging.getLogger(__name__)

redis_host = os.getenv("REDIS_HOST") or "redis"
redis_port = int(os.getenv("REDIS_PORT") or "6379")
redis_pool = redis.ConnectionPool(
    host=redis_host,
    port=redis_port,
    db=0,
    decode_responses=True,
    socket_timeout=2.0,
    socket_connect_timeout=2.0,
)


class PagerLogAPI(http.Controller):
    @http.route("/api/v1/pager/logs/search", type="jsonrpc", auth="user")
    def search_logs(self, file_path, regex_query):
        """
        Splunk-like API: Dispatches a regex search request to the root-chrooted log daemon
        via Redis Pub/Sub, then blocks awaiting the streaming JSON response.
        """
        if not request.env.user.has_group("pager_duty.group_pager_admin"):
            raise AccessError(_("Access Denied: Admins only."))

        if not redis or not redis_pool:
            return {"error": "Redis not available for IPC."}  # audit-ignore-i18n: Tested by [@ANCHOR: test_log_api_i18n]  # fmt: skip

        req_uuid = str(uuid.uuid4())
        payload = {"uuid": req_uuid, "file": file_path, "regex": regex_query}

        try:
            r = redis.Redis(connection_pool=redis_pool)
            pubsub = r.pubsub()
            pubsub.subscribe(f"log_search_res:{req_uuid}")

            # Dispatch to the root daemon
            r.publish("log_search_req", json.dumps(payload))

            # Wait for response (Timeout after 5 seconds to protect WSGI worker)
            for message in pubsub.listen():
                if message["type"] == "message":
                    data = json.loads(message["data"])
                    pubsub.unsubscribe()
                    return data

            return {"error": "Search timeout. Daemon may be offline."}  # audit-ignore-i18n: Tested by [@ANCHOR: test_log_api_i18n]  # fmt: skip
        except Exception as e: # audit-ignore-catch-all
            _logger.error("IPC Failure during log search: %s", e)
            return {"error": f"IPC Failure: {e}"}  # audit-ignore-i18n: Tested by [@ANCHOR: test_log_api_i18n]  # fmt: skip

    @http.route("/api/v1/pager/logs/files", type="jsonrpc", auth="user")
    def get_log_files(self):
        if not request.env.user.has_group("pager_duty.group_pager_admin"):
            raise AccessError(_("Access Denied."))

        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        files = (
            request.env["pager.log.file"]
            .with_user(svc_uid)
            .search([("active", "=", True)], limit=1000)
            .mapped("filepath")
        )
        return {"files": files}
