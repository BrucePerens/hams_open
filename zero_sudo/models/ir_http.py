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

    # [@ANCHOR: zero_sudo:_is_service_account_cached]
    @api.model
    @distributed_cache()
    def _is_service_account_cached(self, uid):
        self.env.cr.execute( # Tested by [@ANCHOR: zero_sudo:COMM_test_is_service_account_cached]
            "SELECT is_service_account FROM res_users WHERE id = %s",
            (uid,)
        )
        res = self.env.cr.fetchone()
        return bool(res and res[0])

    @api.model
    # [@ANCHOR: zero_sudo:is_rpc_path_exempt_from_service_account_block]
    def _is_rpc_path_exempt_from_service_account_block(self, path):
        # bug-hunt (2026-09-13): a bare .startswith('/jsonrpc') /
        # .startswith('/xmlrpc') matched by PREFIX with no delimiter
        # boundary, so a hypothetical future route sharing that prefix
        # without actually being one of Odoo's two real RPC endpoints
        # (exactly '/jsonrpc', or '/xmlrpc/...') would silently bypass this
        # check. Confirmed against odoo/addons/rpc/controllers/
        # {jsonrpc,xmlrpc}.py: the real routes are the exact path '/jsonrpc'
        # and the '/xmlrpc/' prefix (with a trailing separator) -- no
        # currently-defined route in this codebase collides today (grepped
        # repo-wide), so this was latent, not live, but cheap to close
        # outright. Split out as its own small, pure, directly-unit-testable
        # method rather than inlined string logic buried inside
        # `_authenticate` (which needs a real HTTP request context to
        # exercise at all).
        return path == '/jsonrpc' or path.startswith('/xmlrpc/')

    @classmethod
    # [@ANCHOR: zero_sudo:ir_http_authenticate]
    def _authenticate(cls, endpoint):
        super()._authenticate(endpoint)
        if request.session.uid:
            if request.env["ir.http"]._is_service_account_cached(request.session.uid):
                path = request.httprequest.path
                if not request.env["ir.http"]._is_rpc_path_exempt_from_service_account_block(path):
                    raise AccessError(_("Interactive Web UI access is denied for service accounts."))
