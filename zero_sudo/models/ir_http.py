# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, api, _
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
from odoo.http import request
from odoo.exceptions import AccessError

class IrHttp(models.AbstractModel):
    _inherit = 'ir.http'

    @api.model
    @distributed_cache('uid')
    def _is_service_account_cached(self, uid):
        self.env.cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: zero_sudo:COMM_test_is_service_account_cached]  # fmt: skip
            "SELECT is_service_account FROM res_users WHERE id = %s",
            (uid,)
        )
        res = self.env.cr.fetchone()
        return bool(res and res[0])

    @classmethod
    def _authenticate(cls, endpoint):
        super()._authenticate(endpoint)
        if request.session.uid:
            if request.env["ir.http"]._is_service_account_cached(request.session.uid):
                if not (request.httprequest.path.startswith('/jsonrpc') or request.httprequest.path.startswith('/xmlrpc')):
                    raise AccessError(_("Interactive Web UI access is denied for service accounts."))
