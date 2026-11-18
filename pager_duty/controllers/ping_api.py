# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import json
from odoo import http, fields
from odoo.http import request
from odoo.addons.knowledge.rate_limit import network_hits

# [@ANCHOR: pager_duty:heartbeat_miss_limit]
# Verified by [@ANCHOR: test_heartbeat_misses_are_limited_per_network]
# Every unknown uuid costs a database lookup, so misses are counted per network (/24 or /48) and answered 429 past 600
# an hour. A real heartbeat is never counted or refused. A Redis outage fails OPEN (the miss is a plain 404): a dead-man's
# switch that rejected real heartbeats because the counter was down would raise false alarms.
MISSES_PER_HOUR_NETWORK = 600


# [@ANCHOR: pager_duty_ping_api_pager_ping_api]
class PagerPingAPI(http.Controller):
    @http.route(
        "/api/v1/pager/ping", type="http", auth="public", methods=["GET"], csrf=False
    )
    # [@ANCHOR: pager_duty:ping]
    def ping(self, **kw):
        return http.Response(
            json.dumps({"status": "ok"}), content_type="application/json"
        )

    @http.route(
        "/api/v1/pager/heartbeat/<string:hb_uuid>",
        type="http",
        auth="public",
        methods=["POST", "GET"],
        csrf=False,
    )
    # [@ANCHOR: pager_duty:heartbeat]
    # GET may write and csrf is off by design: the uuid in the URL is the secret, like any dead-man's-switch ping URL.
    def heartbeat(self, hb_uuid, **kw):

        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        check_id = request.env["pager.check"]._get_check_id_by_uuid(
            hb_uuid, override_svc_uid=svc_uid
        )
        if check_id:
            check = request.env["pager.check"].with_user(svc_uid).with_context(mail_notrack=True).browse(check_id)
            check.last_heartbeat = fields.Datetime.now()
            return http.Response(
                json.dumps({"status": "ok"}), content_type="application/json"
            )
        misses = network_hits(request.env, "pager_heartbeat_miss", 3600)
        if misses is not None and misses > MISSES_PER_HOUR_NETWORK:
            return http.Response(
                json.dumps({"err": "too many unknown heartbeats from your network"}),
                status=429,
                content_type="application/json",
            )
        return http.Response(
            json.dumps({"err": "not found"}),
            status=404,
            content_type="application/json",
        )
