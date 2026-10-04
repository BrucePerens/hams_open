# SPDX-License-Identifier: AGPL-3.0-or-later
"""Decides, for a request that arrived through Cloudflare, which kind of site it is for.

main     the main site's own hostnames (and its members' custom domains): served exactly as before.
tenant   a hostname of a tenant website: that website only, read-only, public content only.
parking  a parked domain (the parking module): answered from a table.
unknown  any other hostname: a 404 that is never cached.
reject   the Host the client sent and the one Odoo would use disagree (X-Forwarded-Host): a 400.

Only a request whose Host is a real domain name is classified. A request for `localhost`, an IP address
or a one-word name (this machine's daemons calling the JSON-2 API, the test harness, an operator through
an SSH tunnel) is always `main`, unless it carries Cloudflare's own headers, which a request through the
tunnel always does: then a Host that is not a domain name is refused (400)."""

import logging

from werkzeug.exceptions import MethodNotAllowed, NotFound
from werkzeug.wrappers import Response

from odoo import models
from odoo.http import request

from .. import request_state, utils

_logger = logging.getLogger(__name__)

KIND_MAIN = "main"
KIND_TENANT = "tenant"
KIND_UNKNOWN = "unknown"
KIND_REJECT = "reject"
PLAIN_MARKER = "X-Tenant-Sites-Plain"
# No route has a path like this; routing it gives a refused request the state of an unknown page.
UNROUTABLE_PATH = "/_tenant_sites_/no/route/here"


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    # [@ANCHOR: tenant_sites:COMM_classify]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_classify]
    @classmethod
    def _tenant_classify(cls):
        """The kind of the current request, computed once per request."""
        memo = request_state.state()
        if "kind" not in memo:
            memo["kind"] = cls._tenant_compute_kind()
        return memo["kind"]

    @classmethod
    def _tenant_compute_kind(cls):
        httprequest = request.httprequest
        environ = httprequest.environ
        host = utils.normalize_host(utils.original_host(environ))
        if not host:
            return KIND_REJECT if utils.through_cloudflare(httprequest.headers) else KIND_MAIN
        if host != utils.normalize_host(environ.get("HTTP_HOST", "")):
            return KIND_REJECT
        hosts = request.env["tenant.site.host"]
        own_patterns = hosts._own_host_patterns()
        if utils.host_matches(host, own_patterns):
            return KIND_MAIN
        website_id = hosts._website_id_for_host(host)
        if website_id:
            request_state.state()["website_id"] = website_id
            return KIND_TENANT
        extra_kind = request.env["ir.http"]._tenant_extra_kind(host)
        if extra_kind:
            return extra_kind
        member_domains = hosts._service_env()["edge.routing.domain"]
        if member_domains.search_count([("name", "=", host)], limit=1):
            return KIND_MAIN
        if not own_patterns:
            return KIND_MAIN
        return KIND_UNKNOWN

    # [@ANCHOR: tenant_sites:COMM_extra_kind]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_extra_kind]
    @classmethod
    def _tenant_extra_kind(cls, host):
        """Hook: a module that serves its own kind of hostname returns that kind's name."""
        return None

    # [@ANCHOR: tenant_sites:COMM_public_route]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_public_route]
    @classmethod
    def _tenant_public_route(cls, kind, path, method):
        """Hook: True when `path` is a real route for a non-site kind (parking's inquiry post)."""
        return False

    # [@ANCHOR: tenant_sites:COMM_match_guard]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_match_guard]
    @classmethod
    def _match(cls, path_info):
        kind = cls._tenant_classify()
        if kind == KIND_MAIN:
            return super()._match(path_info)
        if kind == KIND_TENANT:
            return cls._tenant_match(path_info)
        # Decided BEFORE the router runs: a wrong-method match (GET on a POST-only route) would
        # otherwise surface as 405 and tell a visitor which backend routes exist.
        if cls._tenant_public_route(kind, path_info, request.httprequest.method):
            return super()._match(path_info)
        raise NotFound()

    # [@ANCHOR: tenant_sites:COMM_tenant_match]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_tenant_match]
    @classmethod
    def _tenant_match(cls, path_info):
        """Routes a tenant request. A refusal is always a NotFound that Odoo's website answers as for
        any unknown page (a wrong-method match must never surface as a 405 that tells a visitor which
        backend routes exist)."""
        if request.httprequest.method not in utils.SAFE_METHODS:
            return cls._tenant_refuse()
        if not utils.tenant_path_allowed(path_info):
            return cls._tenant_refuse()
        module_name = cls._tenant_route_module(path_info)
        if module_name is not None and not utils.tenant_module_allowed(path_info, module_name):
            return cls._tenant_refuse()
        return super()._match(path_info)

    # [@ANCHOR: tenant_sites:COMM_route_module]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_route_module]
    @classmethod
    def _tenant_route_module(cls, path_info):
        """The Python module of the controller a path would route to, or None when no route matches.
        Asked of the routing map directly: Odoo's own matching sets up the language and frontend state
        of the request as a side effect, and that must happen only for a path that is really served."""
        website = request.env["website"].with_context(lang=None).get_current_website()
        request.website_routing = website.id  # what the website module's own _match sets first
        adapter = request.env["ir.http"].routing_map().bind_to_environ(request.httprequest.environ)
        try:
            rule, _arguments = adapter.match(path_info=path_info, return_rule=True)
        except (NotFound, MethodNotAllowed):
            return None
        return rule.endpoint.func.original_endpoint.__module__

    # [@ANCHOR: tenant_sites:COMM_refuse]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_refuse]
    @classmethod
    def _tenant_refuse(cls):
        """Let Odoo route a path that has no route (so the request gets the language and frontend state
        the website's page and 404 handling need), then answer NotFound whatever it found."""
        try:
            super()._match(UNROUTABLE_PATH)
        except NotFound:
            _logger.debug("tenant request %s refused", request.httprequest.path)
        raise NotFound()

    # [@ANCHOR: tenant_sites:COMM_serve_fallback]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_serve_fallback]
    @classmethod
    def _serve_fallback(cls):
        kind = cls._tenant_classify()
        if kind == KIND_TENANT and request.httprequest.method not in utils.SAFE_METHODS:
            return cls._tenant_plain_response("not found", 404)
        if kind in (KIND_MAIN, KIND_TENANT):
            return super()._serve_fallback()
        return cls._tenant_serve_other(kind)

    # [@ANCHOR: tenant_sites:COMM_serve_redirect]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_serve_redirect]
    @classmethod
    def _serve_redirect(cls):
        """On a tenant website only the tenant's own redirects apply, never one shared by all websites."""
        redirect = super()._serve_redirect()
        website_id = request.env["tenant.site.host"]._tenant_request_website_id()
        if not redirect or not website_id:
            return redirect
        return redirect.filtered(lambda record: record.website_id.id == website_id)

    # [@ANCHOR: tenant_sites:COMM_serve_other]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_serve_other]
    @classmethod
    def _tenant_serve_other(cls, kind):
        """Hook: answers a request of a non-site kind. The default is a plain, uncached refusal."""
        if kind == KIND_REJECT:
            return cls._tenant_plain_response("bad host", 400)
        return cls._tenant_plain_response("unknown host", 404)

    # [@ANCHOR: tenant_sites:COMM_plain_response]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_plain_response]
    @classmethod
    def _tenant_plain_response(cls, body, status):
        response = Response(body, status=status, content_type="text/plain; charset=utf-8")
        response.headers[PLAIN_MARKER] = "1"
        response.headers["Cache-Control"] = "no-store"
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["X-Robots-Tag"] = "noindex, nofollow"
        return response

    # [@ANCHOR: tenant_sites:COMM_post_dispatch]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_post_dispatch]
    @classmethod
    def _post_dispatch(cls, response):
        super()._post_dispatch(response)
        if response.headers.get(PLAIN_MARKER):
            # Odoo hands every cookieless visitor a fresh session_id; a response that is not part
            # of a site must never carry a Set-Cookie.
            response.headers.setlist("Set-Cookie", [])
            response.headers.setlist(PLAIN_MARKER, [])
