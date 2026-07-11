# -*- coding: utf-8 -*-
from odoo import models
from odoo.http import request
from odoo.exceptions import AccessError

class IrHttp(models.AbstractModel):
    _inherit = 'ir.http'

    @classmethod
    def _authenticate(cls, endpoint):
        super()._authenticate(endpoint)
        if request.session.uid:
            request.env.cr.execute(
                "SELECT is_service_account FROM res_users WHERE id = %s",
                (request.session.uid,)
            )
            res = request.env.cr.fetchone()
            if res and res[0]:
                if request.httprequest.path.startswith('/jsonrpc') or request.httprequest.path.startswith('/xmlrpc'):
                    raise AccessError("RPC access is denied for service accounts.")
