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

Trusting the peer: loopback (this deployment's own Cloudflare Tunnel case -- `cloudflared` and
Odoo run on the same host, so the peer is always 127.0.0.1/::1) is checked first and needs no
database or cache access at all, exactly as before. For a self-hosted admin running Cloudflare
WITHOUT Tunnel (Cloudflare's edge connecting to the origin directly over the network), the peer is
instead checked against `cloudflare.trusted_ip_ranges`'s admin-configurable allow-list (Settings ->
Cloudflare), read from Redis with no `env` needed (see that module's own docstring for why) since
no Odoo `env`/cursor exists yet at this point in the request. Redis being unreachable, or the admin
never having configured anything, both fall back to that module's own baked-in Cloudflare-range
snapshot -- this hook never simply trusts an unrecognized peer.
"""
import ipaddress
import json
import logging
import threading
import time

import odoo.http
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection
from .models.trusted_ip_ranges import (
    DEFAULT_CLOUDFLARE_IPV4_RANGES,
    DEFAULT_CLOUDFLARE_IPV6_RANGES,
    REDIS_KEY,
)

_logger = logging.getLogger(__name__)

_TRUSTED_LOOPBACK_ADDRS = ("127.0.0.1", "::1")  # burn-ignore-tunnel-peer-check

# Re-reading Redis on every single request would add a network round trip to the hot path for
# every non-Tunnel visitor; a short TTL keeps a settings change or cron refresh visible within a
# minute without that per-request cost. Module-level and per-worker-process on purpose -- each
# pre-forked Odoo worker (real production runs with workers>0) keeps its own independent copy.
_RANGES_CACHE_TTL_SECONDS = 60
_ranges_cache_lock = threading.Lock()
_ranges_cache = {"networks": (), "loaded_at": 0.0}


def _default_trusted_networks():
    return tuple(
        ipaddress.ip_network(cidr)
        for cidr in DEFAULT_CLOUDFLARE_IPV4_RANGES + DEFAULT_CLOUDFLARE_IPV6_RANGES
    )


def _get_cached_trusted_networks():
    now = time.monotonic()
    with _ranges_cache_lock:
        if now - _ranges_cache["loaded_at"] < _RANGES_CACHE_TTL_SECONDS:
            return _ranges_cache["networks"]
    networks = _default_trusted_networks()
    try:
        raw = get_redis_connection().get(REDIS_KEY)
        if raw:
            networks = tuple(ipaddress.ip_network(cidr) for cidr in json.loads(raw))
    except Exception as e:  # audit-ignore-catch-all
        # Redis down, key missing/expired, or malformed content -- fall back to the baked-in
        # default rather than let a cache-refresh failure block every non-Tunnel request.
        _logger.info("Using default Cloudflare trusted IP ranges (Redis unavailable: %s)", e)
    with _ranges_cache_lock:
        _ranges_cache["networks"] = networks
        _ranges_cache["loaded_at"] = now
    return networks


def _is_trusted_cf_peer(remote_addr):
    # [@ANCHOR: cloudflare:is_trusted_cf_peer]
    if remote_addr in _TRUSTED_LOOPBACK_ADDRS:
        return True
    try:
        peer = ipaddress.ip_address(remote_addr)
    except ValueError:
        return False
    return any(peer in network for network in _get_cached_trusted_networks())


_original_application_call = odoo.http.Application.__call__


def _patched_application_call(self, environ, start_response, *args, **kwargs):
    # [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    # Trust CF-Visitor only from a peer that is either loopback (this deployment's own Tunnel
    # case) or in the admin-configured Cloudflare IP allow-list (the non-Tunnel case) -- see
    # cloudflare/models/edge_context.py's own get_request_context(), which applies the identical
    # trust check for the same reason: an untrusted peer could otherwise forge any CF-* header it
    # likes. A forged CF-Visitor from a trusted-looking-but-wrong peer can only make this code
    # wrongly believe an actually-plain-HTTP connection is HTTPS; the practical effect would be
    # the browser on the other end rejecting the resulting Secure-flagged cookie outright
    # (browsers refuse to store a Secure cookie set over a response that didn't itself arrive
    # over HTTPS) -- not a real vulnerability, but there is no reason to skip this check.
    if (
        _is_trusted_cf_peer(environ.get("REMOTE_ADDR"))
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
