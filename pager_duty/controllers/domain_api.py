# -*- coding: utf-8 -*-
from odoo import http, _
from odoo.http import request


class PagerDutyController(http.Controller):

    @http.route(
        "/api/v1/pager_duty/update_domains",
        type="jsonrpc",
        auth="public",
        methods=["POST"],
        csrf=False,
    )
    def update_domains(self, **kwargs):
        """
        Receives a list of custom domains and updates the pager duty maintenance function.
        """
        payload = request.jsonrequest
        if not payload:
            return {"status": "error", "message": _("Empty payload")}

        domains = payload.get("domains", [])
        # Call the model method to handle this using a service account
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        request.env["pager.check"].with_user(svc_uid).update_lets_encrypt_domains(
            domains
        )
        return {"status": "success"}
