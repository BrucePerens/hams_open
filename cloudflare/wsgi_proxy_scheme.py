# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""
[@ANCHOR: cloudflare:wsgi_proxy_scheme_fix]
Verified by [@ANCHOR: test_wsgi_proxy_scheme_fix_sets_https_for_trusted_cf_visitor]

night_shift_todo's own "Session cookie has no Secure attribute" investigation found the real,
complete root cause: `request.httprequest.scheme` never reports "https" behind this deployment's
real edge (Cloudflare Tunnel), because Cloudflare sends `CF-Visitor: {"scheme":"https"}` on the
origin request, not the `X-Forwarded-Proto` header Odoo's own `proxy_mode`/`ProxyFix` handling
actually reads (confirmed against real cloudflared source, `RELEASE_NOTES:951`/TUN-5744: cloudflared
has no config option to send a custom `X-Forwarded-Proto` header at all, by design). Every place in
Odoo that trusts `request.httprequest.scheme` -- not just the session-cookie `Secure` flag this was
originally found from, but absolute-URL generation and redirect handling too -- is wrong on this
deployment as a result.

This module fixes it at the one place all of those readers ultimately derive from: the raw WSGI
`environ["wsgi.url_scheme"]` value, which Werkzeug's `Request.scheme` (and therefore
`request.httprequest.scheme`) reads directly at construction time
(`werkzeug/wrappers/request.py`: `scheme=environ.get("wsgi.url_scheme", "http")`). Odoo's own
`ProxyFix` handling is not used or relied on here at all -- it is itself gated on
`environ.get("HTTP_X_FORWARDED_HOST")` being present (`odoo/http.py`'s `Application.__call__`),
and confirmed directly against this project's own local Cloudflare Tunnel simulator
(`hams_open/daemons/cloudflared-ffi/main.go`, built specifically to mimic real Cloudflare edge
behavior for testing) that `X-Forwarded-Host` is never sent alongside `CF-Visitor` either -- relying
on `ProxyFix` would have silently done nothing.

Monkeypatches `odoo.http.Application.__call__` (the actual WSGI entry point, `odoo.http.root`'s own
`__call__`) at import time, the same technique `zero_sudo`'s own test harness already uses for
comparable "patch a stock Odoo/Werkzeug internal for a cross-cutting reason" cases -- there is no
formal WSGI-middleware extension point for Odoo addons, so this is the only way to run code this
early, before Odoo's own routing/dispatch (and therefore before every reader of
`request.httprequest.scheme`) ever sees the request.
"""
import json
import logging

import odoo.http

_logger = logging.getLogger(__name__)

_TRUSTED_LOOPBACK_ADDRS = ("127.0.0.1", "::1")  # burn-ignore-tunnel-peer-check

_original_application_call = odoo.http.Application.__call__


def _patched_application_call(self, environ, start_response, *args, **kwargs):
    # [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    # Only trust CF-Visitor when the connection's real transport peer is loopback --
    # cloudflared and Odoo run on the same host on this deployment (see
    # cloudflare/models/edge_context.py's own get_request_context(), which applies the
    # identical "CF-* headers are only genuine from loopback" check for the same reason:
    # this deployment is Tunnel-only, so a direct, non-tunneled request could otherwise forge
    # any CF-* header it likes). A forged CF-Visitor on a direct connection can only make this
    # code wrongly believe an actually-plain-HTTP connection is HTTPS; the practical effect
    # would be the browser on the other end rejecting the resulting Secure-flagged cookie
    # outright (browsers refuse to store a Secure cookie set over a response that didn't
    # itself arrive over HTTPS) -- not a real vulnerability, but there is no reason to skip
    # the same trust check this codebase already established for every other CF-* header.
    if (
        environ.get("REMOTE_ADDR") in _TRUSTED_LOOPBACK_ADDRS
        and not environ.get("HTTP_X_FORWARDED_PROTO")
    ):
        cf_visitor_raw = environ.get("HTTP_CF_VISITOR")
        if cf_visitor_raw:
            scheme = None
            try:
                scheme = json.loads(cf_visitor_raw).get("scheme")
            except (ValueError, AttributeError) as e:
                _logger.info("Ignoring malformed CF-Visitor header %r: %s", cf_visitor_raw, e)
            if scheme in ("http", "https"):
                environ["wsgi.url_scheme"] = scheme
                environ["HTTP_X_FORWARDED_PROTO"] = scheme
    return _original_application_call(self, environ, start_response, *args, **kwargs)


odoo.http.Application.__call__ = _patched_application_call
