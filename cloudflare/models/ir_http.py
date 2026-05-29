# -*- coding: utf-8 -*-
from odoo import models
from odoo.http import request


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _post_dispatch(cls, response):
        # [@ANCHOR: ir_http_post_dispatch_headers]
        """
        Intercepts the outgoing HTTP response to inject CDN caching directives.
        Generalized to dynamically check Odoo's public user state without requiring
        strict app dependencies.
        """
        res = super()._post_dispatch(response)

        if not hasattr(response, "headers"):
            return res

        try:
            if not request:
                return res

            request_obj = request._get_current_object()
            path = request_obj.httprequest.path
        except (RuntimeError, AttributeError):
            return res

        # 1. Media & Assets (Max aggressive caching: 1 year)
        # CRITICAL: /web/image and /web/content MUST NOT be aggressively cached here,
        # as it bypasses Odoo ACLs and causes Edge Cache IDORs for private attachments.
        if any(path.startswith(prefix) for prefix in ("/web/static", "/web/assets")): # burn-ignore-route
            response.headers["Cloudflare-CDN-Cache-Control"] = "max-age=31536000"
            response.headers["Cache-Tag"] = "odoo-static-assets"
            return res

        # 2. Hardcoded Dynamic or API Routes (Zero caching)
        # [@ANCHOR: cf_nocache_routes]
        if any(path.startswith(prefix) for prefix in ("/my/", "/web/", "/api/", "/shop/cart", "/shop/checkout", "/shop/confirm_order", "/helpdesk/")): # burn-ignore-route
            response.headers["Cloudflare-CDN-Cache-Control"] = "no-cache, no-store"
            return res

        # 3. Dynamic State Isolation (Protecting Authenticated User Data)
        is_public = True
        if (
            getattr(request, "env", False)
            and getattr(request.env, "user", False)
            and request.env.user
        ):
            is_public = request.env.user._is_public()

        if not is_public:
            response.headers["Cloudflare-CDN-Cache-Control"] = "no-cache, no-store"
            return res

        # 4. Semi-Static Content (Public Website Pages, Blogs, Classifieds)
        # Cache heavily at the edge. The purge_queue will invalidate individual URLs when edited.
        response.headers["Cloudflare-CDN-Cache-Control"] = "max-age=86400"
        return res
