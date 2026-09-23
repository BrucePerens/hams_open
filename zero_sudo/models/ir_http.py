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

# [@ANCHOR: zero_sudo:hardened_cookie_names]
# night_shift_todo's own "session-cookie-missing-secure-and-samesite-attributes"
# finding, 2026-09-22: Odoo's own session_id cookie (the real authentication
# token) and frontend_lang carry HttpOnly but neither Secure nor SameSite, even
# when the request genuinely arrived over HTTPS via the reverse proxy
# (X-Forwarded-Proto correctly reflected through proxy_mode). Scoped to exactly
# these two cookie names, matching that investigation's own documented scope --
# a cookie set explicitly elsewhere with its own deliberate flags (e.g.
# gdpr_export_token, set by user_websites' privacy_export_zip() with its own
# explicit secure=/httponly=/samesite= already) is left untouched here.
_HARDENED_COOKIE_NAMES = {"session_id", "frontend_lang"}


# [@ANCHOR: zero_sudo:harden_cookie_header]
def _harden_cookie_header(cookie_header, is_https):
    """Adds Secure (only when the request was actually HTTPS) and SameSite=Lax
    to one raw Set-Cookie header value, if its cookie name is in
    _HARDENED_COOKIE_NAMES and it doesn't already carry that attribute. Pure
    string logic, split out of _post_dispatch so it's directly unit-testable
    without a real HTTP request context.

    SameSite=Lax rather than Strict: this site's own LoTW passwordless-login
    and invite-redemption flows both land back on hams.com via a cross-site
    redirect from a third party (LoTW's own auth gateway) -- Strict would
    drop the session cookie on exactly that first redirected request, the
    same well-known SameSite=Strict-breaks-cross-site-auth-redirects failure
    this codebase's own finding already flagged to check for.
    """
    name = cookie_header.split("=", 1)[0].strip()
    if name not in _HARDENED_COOKIE_NAMES:
        return cookie_header
    parts = [p.strip() for p in cookie_header.split(";")]
    attrs_lower = {p.split("=")[0].strip().lower() for p in parts[1:]}
    if is_https and "secure" not in attrs_lower:
        parts.append("Secure")
    if "samesite" not in attrs_lower:
        parts.append("SameSite=Lax")
    return "; ".join(parts)


class IrHttp(models.AbstractModel):
    _inherit = 'ir.http'

    @classmethod
    # [@ANCHOR: zero_sudo:ir_http_post_dispatch_cookie_hardening]
    def _post_dispatch(cls, response):
        res = super()._post_dispatch(response)
        if not request:
            return res
        # request.httprequest.scheme already correctly reflects
        # X-Forwarded-Proto under this deployment's own proxy_mode=True --
        # the same established pattern user_websites/controllers/main.py's
        # own privacy_export_zip() cookie already uses (`secure=request.
        # httprequest.scheme == "https"`), confirmed correct there.
        is_https = request.httprequest.scheme == "https"
        cookie_headers = response.headers.getlist("Set-Cookie")
        if not cookie_headers:
            return res
        hardened = [_harden_cookie_header(h, is_https) for h in cookie_headers]
        if hardened != cookie_headers:
            # Real bug found live in production, 2026-09-23: `del
            # response.headers["Set-Cookie"]` raises `AttributeError:
            # __delitem__` on this runtime's actual response.headers object
            # (Response.headers goes through this codebase's own
            # odoo/tools/facade.py wrapping, which doesn't forward the
            # dunder `__delitem__` the way plain attribute access forwards
            # ordinary methods) -- confirmed directly in a real `odoo shell`
            # against a real odoo.http.Response object, reproducing the
            # exact site-wide 500 this caused on every single page (every
            # response carries a session_id cookie). setlist() replaces the
            # whole header in one call and has no such gap.
            response.headers.setlist("Set-Cookie", hardened)
        return res

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
