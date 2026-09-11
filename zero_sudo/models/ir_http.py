# -*- coding: utf-8 -*-
from odoo import models, api, _
from odoo.http import request
from odoo.exceptions import AccessError
# distributed_redis_cache depends on zero_sudo, so zero_sudo can't declare a real
# 'depends' entry back on it without closing a cycle -- see this module's own
# 'depends_cycle' manifest entry and zero_sudo.security.utils._resolve_dependency_cycle's
# docstring for the established convention. This import is still a real, unconditional
# coupling (the @distributed_cache() decorator below needs the name at class-definition
# time, so a lazy runtime-guarded import like _resolve_dependency_cycle's usual callers
# use doesn't apply here); it works because Python resolves `odoo.addons.X` against the
# addons path directly, independent of either module's per-database installation state.
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache

class IrHttp(models.AbstractModel):
    _inherit = 'ir.http'

    @api.model
    @distributed_cache()
    def _is_service_account_cached(self, uid):
        self.env.cr.execute( # Tested by [@ANCHOR: zero_sudo:COMM_test_is_service_account_cached]
            "SELECT is_service_account FROM res_users WHERE id = %s",
            (uid,)
        )
        res = self.env.cr.fetchone()
        return bool(res and res[0])

    @classmethod
    # [@ANCHOR: zero_sudo:ir_http_authenticate]
    def _authenticate(cls, endpoint):
        super()._authenticate(endpoint)
        if request.session.uid:
            if request.env["ir.http"]._is_service_account_cached(request.session.uid):
                if not (request.httprequest.path.startswith('/jsonrpc') or request.httprequest.path.startswith('/xmlrpc')):
                    raise AccessError(_("Interactive Web UI access is denied for service accounts."))
