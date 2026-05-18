# -*- coding: utf-8 -*-
import json
import uuid
import os
import time
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
        # [@ANCHOR: pd_log_api_i18n]
        if not request.env.user.has_group("pager_duty.group_pager_admin"):
            raise AccessError(_("Access Denied: Admins only."))

        if not redis or not redis_pool:
            return {"error": _("Redis not available for IPC.")}  # # Tests [@ANCHOR: pd_log_api_i18n]  # fmt: skip

        req_uuid = str(uuid.uuid4())
        payload = {"uuid": req_uuid, "file": file_path, "regex": regex_query}

        try:
            r = redis.Redis(connection_pool=redis_pool)
            pubsub = r.pubsub()
            pubsub.subscribe(f"log_search_res:{req_uuid}")

            # Dispatch to the root daemon
            r.publish("log_search_req", json.dumps(payload))

            # Wait for response (Timeout after 5 seconds to protect WSGI worker)
            # BLOCKING CALL: get_message with timeout is the optimized pattern for Odoo WSGI workers
            # as it yields the thread until Redis activity or timeout occurs.
            message = pubsub.get_message(ignore_subscribe_messages=True, timeout=5.0)
            if message and message["type"] == "message":
                data = json.loads(message["data"])
                pubsub.unsubscribe()
                return data

            pubsub.unsubscribe()
            return {"error": _("Search timeout. Daemon may be offline.")}  # # Tests [@ANCHOR: pd_log_api_i18n]  # fmt: skip
        except redis.RedisError as e:
            _logger.error("Redis IPC Failure during log search: %s", e)
            return {"error": _("IPC Failure: %s") % e}  # # Tests [@ANCHOR: pd_log_api_i18n]  # fmt: skip
        except json.JSONDecodeError as e:
            _logger.error("JSON Decode Failure during log search: %s", e)
            return {"error": _("Protocol Error: Failed to parse response.")}
        except Exception as e: # audit-ignore-catch-all
            _logger.error("Unexpected Failure during log search: %s", e)
            return {"error": _("An unexpected error occurred.")}

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
