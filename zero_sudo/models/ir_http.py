# -*- coding: utf-8 -*-
from odoo import models, api, _
from odoo.http import request
from odoo.exceptions import AccessError
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache

class IrHttp(models.AbstractModel):
    _inherit = 'ir.http'

    @api.model
    @distributed_cache()
    def _is_service_account_cached(self, uid):
        self.env.cr.execute(
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
