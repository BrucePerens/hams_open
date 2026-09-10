# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import json
import uuid
import os
import logging
from odoo import http, _
from odoo.http import request
from odoo.exceptions import AccessError

from odoo.addons.distributed_redis_cache.redis_pool import redis, redis_pool

_logger = logging.getLogger(__name__)


class PagerLogAPI(http.Controller):
    @http.route("/api/v1/pager/logs/search", type="jsonrpc", auth="user")
    # [@ANCHOR: pager_duty:search_logs]
    def search_logs(self, file_path, regex_query):
        """
        Splunk-like API: Dispatches a regex search request to the root-chrooted log daemon
        via Redis Pub/Sub, then blocks awaiting the streaming JSON response.
        """
        # [@ANCHOR: pd_log_api_i18n]
        if not request.env.user.has_group("pager_duty.group_pager_admin"):
            raise AccessError(_("Access Denied: Admins only."))

        # CWE-22 Path Traversal Prevention
        if ".." in file_path.split(os.path.sep):
            raise AccessError(_("Illegal path traversal detected."))

        base_dir = os.path.realpath("/var/log")
        real_path = os.path.realpath(file_path)
        if real_path != base_dir and not real_path.startswith(base_dir + os.path.sep):
            raise AccessError(_("Illegal path traversal detected."))

        # Bug-hunt fix (class 17: iteration/reach broader than granted
        # scope): being under /var/log was the ONLY check on file_path --
        # any group_pager_admin member could search ANY file under
        # /var/log, including another tenant's own log files sharing this
        # host, not just files their own website/company actually
        # registered via pager.log.file. request.env is the CALLING
        # user's own env, so this search already applies
        # pager_log_file_website_company_rule (the same ir.rule this
        # module tests elsewhere as test_04_multi_tenant_isolation/
        # test_05_multi_website_isolation) -- only a path the caller's own
        # scope actually registered is allowed through.
        allowed_paths = {
            os.path.realpath(p)
            for p in request.env["pager.log.file"].search([("active", "=", True)]).mapped("filepath")
        }
        if real_path not in allowed_paths:
            raise AccessError(_("File is not a registered pager_duty log target for your scope."))

        if not redis or not redis_pool:
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_log_api_i18n]  # fmt: skip
            return {"error": _("Redis not available for IPC.")}

        req_uuid = str(uuid.uuid4())
        payload = {"uuid": req_uuid, "file": file_path, "regex": regex_query}

        # Asynchronous Bastion Pattern: State Initialization
        request.env["pager.log.search.job"].create({
            "uuid": req_uuid,
            "state": "pending"
        })

        try:
            r = redis.Redis(connection_pool=redis_pool)
            # Transactional Dispatch to Daemon
            r.publish("log_search_req", json.dumps(payload))
            return {"job_id": req_uuid}
        except redis.RedisError as e:
            _logger.error("Redis IPC Failure during log search: %s", e)
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_log_api_i18n]  # fmt: skip
            return {"error": _("IPC Failure: %s") % e}
        except json.JSONDecodeError as e:
            _logger.error("JSON Decode Failure during log search: %s", e)
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_log_api_i18n]  # fmt: skip
            return {"error": _("Protocol Error: Failed to parse response.")}

    @http.route("/api/v1/pager/logs/search_poll", type="jsonrpc", auth="user")
    # [@ANCHOR: pager_duty:search_logs_poll]
    def search_logs_poll(self, job_id):
        if not request.env.user.has_group("pager_duty.group_pager_admin"):
            raise AccessError(_("Access Denied."))
        
        job = request.env["pager.log.search.job"].search([("uuid", "=", job_id)], limit=1)
        if not job:
            return {"error": "Job not found"}
            
        if job.state == "pending":
            return {"status": "pending"}
        elif job.state == "error":
            return {"error": job.result_payload or "Daemon processing error"}
        else:
            try:
                data = json.loads(job.result_payload) if job.result_payload else {}
                return {"status": "done", "matches": data.get("matches", [])}
            except json.JSONDecodeError:
                return {"error": "Invalid result payload"}

    @http.route("/api/v1/pager/logs/files", type="jsonrpc", auth="user")
    # [@ANCHOR: pager_duty:get_log_files]
    def get_log_files(self):
        if not request.env.user.has_group("pager_duty.group_pager_admin"):
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_log_api_i18n]  # fmt: skip
            raise AccessError(_("Access Denied."))

        # Bug-hunt fix (class 17): this used to elevate to the pager_duty
        # service account -- whose own pager_log_file_website_rule_svc
        # ir.rule is deliberately unrestricted, domain [(1,'=',1)] -- to
        # list files, which handed EVERY tenant's registered log file
        # paths to any group_pager_admin member, not just the caller's
        # own website/company. Using the caller's own env (no with_user
        # elevation) applies pager_log_file_website_company_rule instead,
        # so only the files the caller is actually scoped to see come
        # back -- consistent with search_logs()'s own scoping above.
        files = (
            request.env["pager.log.file"]
            .search([("active", "=", True)], limit=1000)
            .mapped("filepath")
        )
        return {"files": files}
