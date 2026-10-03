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

Correction, 2026-09-28: capturing a real request off hams.com's loopback showed the real edge sends
`X-Forwarded-Proto: https` as well as `Cf-Visitor` (the local simulator sends only the latter).
Odoo's ProxyFix does not read it without `X-Forwarded-Host`, so it is applied here too, and it wins
over CF-Visitor when both are present.

Trusting the peer: loopback (this deployment's own Cloudflare Tunnel case -- `cloudflared` and
Odoo run on the same host, so the peer is always 127.0.0.1/::1) is checked first and needs no
database or cache access at all, exactly as before. For a self-hosted admin running Cloudflare
WITHOUT Tunnel (Cloudflare's edge connecting to the origin directly over the network), the peer is
instead checked against `cloudflare.trusted_ip_ranges`'s admin-configurable allow-list (Settings ->
Cloudflare), read from Redis with no `env` needed (see that module's own docstring for why) since
no Odoo `env`/cursor exists yet at this point in the request.

Bug-hunt fix (night_shift_todo/low/cloudflare-trusted-ip-allow-list-defaults-to-cloudflare-ranges-
on-tunnel-only-deployments-7d2b8f14.md): this used to fall back to Cloudflare's own baked-in
published-range snapshot whenever Redis was unreachable, regardless of whether an admin had ever
turned the non-Tunnel allow-list on. That is wrong for a Tunnel-only deployment -- its origin is
never reachable from Cloudflare's ranges at all, so there was never a reason to trust them, Redis
up or down. The cache below now starts empty (loopback-only, correct for a Tunnel-only deployment
and for any Tunnel-only self-hoster who has never touched this setting) and, once a real admin-
published list has been read from Redis at least once, keeps serving that last-known-good list
across a later Redis outage instead of reverting to the Cloudflare-wide snapshot -- this hook never
trusts more than an admin has actually configured, and a transient Redis blip degrades a
non-Tunnel admin's setup to its last good state rather than to either extreme.
"""
import ipaddress
import json
import logging
import threading
import time

import odoo.http
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection
from .models.trusted_ip_ranges import REDIS_KEY

_logger = logging.getLogger(__name__)

_TRUSTED_LOOPBACK_ADDRS = ("127.0.0.1", "::1")  # burn-ignore-tunnel-peer-check

# Re-reading Redis on every single request would add a network round trip to the hot path for
# every non-Tunnel visitor; a short TTL keeps a settings change or cron refresh visible within a
# minute without that per-request cost. Module-level and per-worker-process on purpose -- each
# pre-forked Odoo worker (real production runs with workers>0) keeps its own independent copy.
#
# Starts empty on purpose: loopback-only (this deployment's default, and correct for any
# Tunnel-only self-hoster) until an admin turns on non-Tunnel mode and Redis has actually
# published a real list at least once -- see this module's own docstring for why a Redis outage
# must never seed this from Cloudflare's own published-range snapshot instead.
_RANGES_CACHE_TTL_SECONDS = 60
_ranges_cache_lock = threading.Lock()
_ranges_cache = {"networks": (), "loaded_at": 0.0}


def _get_cached_trusted_networks():
    now = time.monotonic()
    with _ranges_cache_lock:
        if now - _ranges_cache["loaded_at"] < _RANGES_CACHE_TTL_SECONDS:
            return _ranges_cache["networks"]
        # Last-known-good value, kept unless Redis gives us something newer below -- never reset
        # to a wider default just because this one read failed.
        networks = _ranges_cache["networks"]
    try:
        raw = get_redis_connection().get(REDIS_KEY)
        if raw is not None:
            networks = tuple(ipaddress.ip_network(cidr) for cidr in json.loads(raw))
        # raw is None: no admin has ever turned on non-Tunnel mode (or this is a Tunnel-only
        # deployment, which never needs to). Keep whatever was last cached -- empty at cold start.
    except Exception as e:  # audit-ignore-catch-all
        # Redis down, or malformed content -- keep the last-known-good list (empty at cold start,
        # the correct loopback-only default) rather than let a cache-refresh failure either widen
        # trust to Cloudflare's full published ranges or silently drop an admin's real setup.
        _logger.info("Keeping last-known trusted IP ranges (Redis unavailable: %s)", e)
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
    if _is_trusted_cf_peer(environ.get("REMOTE_ADDR")):
        # A real Cloudflare edge (found live on hams.com, 2026-09-28) sends BOTH `Cf-Visitor` and
        # `X-Forwarded-Proto`. Odoo's own ProxyFix only acts on X-Forwarded-Proto when
        # X-Forwarded-Host is also present, and it never is, so the header alone leaves
        # `wsgi.url_scheme` at "http". An earlier version skipped this whole block whenever
        # X-Forwarded-Proto existed ("never override an upstream value") and therefore did nothing on
        # the real edge. The upstream value still wins over CF-Visitor, but it has to be applied.
        forwarded = (environ.get("HTTP_X_FORWARDED_PROTO") or "").split(",")[0].strip().lower()
        scheme = forwarded if forwarded in ("http", "https") else None
        if scheme is None:
            cf_visitor_raw = environ.get("HTTP_CF_VISITOR")
            if cf_visitor_raw:
                try:
                    scheme = json.loads(cf_visitor_raw).get("scheme")
                except (ValueError, AttributeError) as e:
                    _logger.info("Ignoring malformed CF-Visitor header %r: %s", cf_visitor_raw, e)
        if scheme in ("http", "https"):
            environ["wsgi.url_scheme"] = scheme
            environ.setdefault("HTTP_X_FORWARDED_PROTO", scheme)
    return _original_application_call(self, environ, start_response, *args, **kwargs)


odoo.http.Application.__call__ = _patched_application_call
