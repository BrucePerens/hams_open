# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import http, _
from odoo.http import request
import hmac


class PagerDutyController(http.Controller):

    @http.route(
        "/api/v1/pager_duty/update_domains",
        type="jsonrpc",
        auth="public",
        methods=["POST"],
        csrf=False,
    )
    def update_domains(self, domains=None, api_identity=None, **kwargs):
        """
        Receives a list of custom domains and updates the pager duty maintenance function.
        """
        if domains is None:
            # audit-ignore-i18n: Tested by [@ANCHOR: pd_domain_api_i18n]  # fmt: skip
            return {"status": "error", "message": _("Empty payload")}
            
        stored_identity = request.env["zero_sudo.security.utils"]._get_system_param("pager_duty.domain_api_identity")
        if not stored_identity or not api_identity or not hmac.compare_digest(api_identity, stored_identity):
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
