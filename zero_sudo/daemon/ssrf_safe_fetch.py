# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Pure-stdlib, Odoo-free SSRF-safe HTTP(S) fetch helper.

Lives here (zero_sudo/daemon/, not either consumer's own module) because both real
call sites -- `binary_downloader/models/binary_utils.py` and
`pager_duty/daemon/pager_synthetic_spooler.py` -- already declare a hard Odoo
`depends` on `zero_sudo` (binary_downloader ALSO depends on pager_duty, and
pager_duty documents its own circular-dependency workaround for the reverse
direction in its manifest's `depends_cycle` -- putting this in either of THOSE
two modules would tangle with that existing cycle for no reason, where zero_sudo
is a clean common ancestor neither has to newly depend on). `zero_sudo/daemon/`
already establishes the "plain, Odoo-free, importable-as-a-raw-module" convention
this needs (see `json_rpc_client.py` in this same directory) -- MUST NOT import
anything from `odoo` here: `pager_synthetic_spooler.py` is a real standalone
systemd-managed script (see its own `.service` unit) with no `odoo` package on
its Python path at all, and `hams_shared/tools/check_burn_list.py`'s own
"CRITICAL DAEMON DECOUPLING" rule enforces this mechanically for any `daemon/`
directory.

# Why this module exists: closing a real DNS-rebinding TOCTOU

Both call sites used to validate a download URL's hostname via ONE
`socket.getaddrinfo()` call (rejecting anything that resolves to a
loopback/link-local/private-use/etc. address), then hand the SAME hostname to
`urllib.request.urlopen()`, which performs its OWN, completely separate DNS
resolution moments later to actually connect. Nothing tied the validated address
to the address actually connected to -- a classic DNS-rebinding SSRF bypass: an
attacker who controls the target hostname's DNS (already required to exploit
this, and already within both call sites' own documented threat model, since
that same actor already supplies the URL) can return a safe address for the
validation lookup and an internal/loopback/link-local one (e.g. the cloud-metadata
`169.254.169.254`) for the real connection a moment later, via a short/zero TTL.

The fix: resolve and validate exactly ONCE per hop (the original URL, and again
for wherever each HTTP redirect actually lands -- `urlopen_ssrf_safe()` re-runs
this for every redirect target, not just the first URL, since a redirect is
exactly as exploitable as the initial request), then PIN the real TCP/TLS
connection to that exact validated address. `http.client.HTTPConnection.connect()`
and `HTTPSConnection.connect()` don't hardcode `socket.create_connection` --
`HTTPConnection.__init__` stores it as `self._create_connection`, explicitly "to
allow unit tests to replace it with a suitable mockup" (see its own source) --
this is a real, supported seam, not undocumented internals. Replacing that one
attribute with a callable that dials only the pre-validated address, while
leaving `self.host` (used for the `Host` header, and for HTTPS, the TLS SNI
`server_hostname` + certificate hostname verification) completely untouched,
means: no second DNS lookup ever happens for the actual connection, and the
`Host` header / SNI / certificate-hostname-verification story is identical to an
ordinary, unpinned `urlopen()` call.

No process-global mutable state anywhere in this module: every validated
address list and every pinned connection factory is created fresh per call,
closed over locally -- safe under Odoo's real multi-worker/multi-threaded
request model, unlike the tempting (and rejected) alternative of temporarily
monkeypatching the process-global `socket.getaddrinfo`.
"""

import http.client
import ipaddress
import socket
import urllib.error
import urllib.request
from urllib.parse import urlsplit


class SSRFValidationError(Exception):
    """Raised when a URL's hostname is missing, unresolvable, or resolves to a
    non-public address. Deliberately a plain, Odoo-free exception type -- each
    caller translates it into whatever fits its own context (`UserError` in
    `binary_utils.py`, `ValueError` in `pager_synthetic_spooler.py`) rather than
    this shared module picking one Odoo-flavored type for every caller."""


# [@ANCHOR: zero_sudo:ssrf_safe_fetch_is_ssrf_safe_public_ip]
def is_ssrf_safe_public_ip(ip_obj):
    """Returns True only for an address a server-initiated download has any
    legitimate reason to reach. `is_global` alone already excludes every range
    checked individually below on modern Python -- the individual checks are
    kept explicit so this can't silently widen if a future Python release ever
    changes what `is_global` means, and so the intent (reject loopback,
    link-local -- which covers the AWS/GCP/Azure 169.254.169.254 metadata
    address, RFC 1918/4193 private-use ranges, multicast, "reserved," and
    unspecified/0.0.0.0 addresses) is legible without cross-referencing the
    ipaddress module's own docs."""
    return (
        ip_obj.is_global
        and not ip_obj.is_private
        and not ip_obj.is_loopback
        and not ip_obj.is_link_local
        and not ip_obj.is_multicast
        and not ip_obj.is_reserved
        and not ip_obj.is_unspecified
    )


# [@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]
def resolve_ssrf_safe_addresses(hostname, context):
    """Resolves `hostname` via exactly ONE `socket.getaddrinfo()` call and
    raises `SSRFValidationError` unless every resolved address is safe (see
    `is_ssrf_safe_public_ip`). Fails closed on the first unsafe address found
    (deliberately conservative -- a real connection could pick any of the
    returned addresses, so any one of them being unsafe makes the whole
    resolution unsafe to proceed with) rather than checking the rest.

    Returns the full `getaddrinfo()` result list (`(family, socktype, proto,
    canonname, sockaddr)` tuples) on success, so a caller can pin the real
    connection to these EXACT addresses instead of letting a second,
    independently-timed DNS lookup pick again -- the fix for the DNS-rebinding
    TOCTOU this module exists to close. `context` is a short label (a command
    name, a check name, ...) folded into any raised message, matching both
    real call sites' own pre-existing error-message conventions."""
    if not hostname:
        raise SSRFValidationError(f"Download URL for {context} has no hostname.")
    try:
        addrinfo = socket.getaddrinfo(hostname, None)
    except OSError as e:
        raise SSRFValidationError(
            f"Could not resolve download host for {context}: {e}"
        ) from e
    if not addrinfo:
        raise SSRFValidationError(
            f"Could not resolve download host for {context}: no addresses returned."
        )
    for info in addrinfo:
        sockaddr = info[4]
        try:
            ip_obj = ipaddress.ip_address(sockaddr[0])
        except ValueError:
            continue
        if not is_ssrf_safe_public_ip(ip_obj):
            raise SSRFValidationError(
                f"Download URL for {context} resolves to a non-public address "
                f"({sockaddr[0]}). Refusing to fetch from an "
                f"internal/loopback/link-local network target."
            )
    return addrinfo


# [@ANCHOR: zero_sudo:ssrf_safe_fetch_pinned_create_connection]
# Verified by [@ANCHOR: zero_sudo:test_pinned_create_connection_ignores_a_different_address_a_second_lookup_would_return]
def _pinned_create_connection(validated_addrinfo):
    """Returns a `socket.create_connection`-compatible callable that ONLY ever
    dials the exact addresses `resolve_ssrf_safe_addresses()` already
    validated -- it never performs its own DNS resolution, so nothing between
    validation and connection can substitute a different, unvalidated address.
    Tries each validated address in order (same fallback behavior
    `socket.create_connection`/`getaddrinfo` gives an ordinary, unpinned
    connection when the first resolved address refuses the connection),
    raising the last real error only if every one fails.

    Meant to be assigned to a real `http.client.HTTPConnection` (or
    `HTTPSConnection`) instance's own `_create_connection` attribute -- see
    this module's own docstring for why that attribute is a real, supported
    seam. `address` (the first positional arg `HTTPConnection.connect()`
    passes) is `(self.host, self.port)` -- the ORIGINAL hostname and the real
    requested port; only the port is used here (to build each pinned
    candidate's own connect address), since using the hostname at all would
    reintroduce exactly the second DNS lookup this function exists to avoid.
    """

    def _create_connection(
        address, timeout=socket._GLOBAL_DEFAULT_TIMEOUT, source_address=None
    ):
        _requested_host, requested_port = address
        last_err = None
        for family, socktype, proto, _canonname, sockaddr in validated_addrinfo:
            # `resolve_ssrf_safe_addresses()` calls `getaddrinfo(hostname,
            # None)` with no socktype hint, deliberately (a test pins that
            # exact call signature) -- on glibc that returns a SOCK_STREAM,
            # a SOCK_DGRAM, AND a SOCK_RAW entry per address, all resolving
            # to the identical IP. Skip anything but SOCK_STREAM here:
            # `socket.create_connection()` (the function this replaces)
            # only ever requests/uses SOCK_STREAM, and trying a SOCK_DGRAM
            # entry after a SOCK_STREAM `connect()` fails (e.g. connection
            # refused) would "succeed" immediately -- UDP `connect()` just
            # records a default peer, it doesn't require anything to be
            # listening -- silently handing http.client a UDP socket that
            # then hangs in `getresponse()` until the caller's own timeout,
            # turning a fast, clear connection-refused error into a slow,
            # misleading one.
            if socktype != socket.SOCK_STREAM:
                continue
            pinned_sockaddr = (sockaddr[0], requested_port) + tuple(sockaddr[2:])
            sock = None
            try:
                sock = socket.socket(family, socktype, proto)
                try:
                    if source_address:
                        sock.bind(source_address)
                    if timeout is not socket._GLOBAL_DEFAULT_TIMEOUT:
                        sock.settimeout(timeout)
                    sock.connect(pinned_sockaddr)
                    return sock
                except OSError:
                    sock.close()
                    raise
            except OSError as e:
                last_err = e
                continue
        if last_err is not None:
            raise last_err
        raise OSError(f"No validated addresses to connect to for {_requested_host}")

    return _create_connection


def _make_pinned_http_class(base_cls, validated_addrinfo):
    """Returns a callable with the same call signature `do_open()` uses to
    build its connection (`http_class(host, timeout=..., **extra_kwargs)`) --
    a real `base_cls` instance (e.g. `http.client.HTTPConnection` or
    `HTTPSConnection`) whose `_create_connection` has been swapped for the
    pinned version above. `host` is passed through completely unmodified, so
    `self.host`/`self.port` (Host header, TLS SNI, certificate hostname
    verification) are exactly what an ordinary, unpinned connection would use."""

    def _factory(host, timeout=socket._GLOBAL_DEFAULT_TIMEOUT, **kwargs):
        conn = base_cls(host, timeout=timeout, **kwargs)
        conn._create_connection = _pinned_create_connection(validated_addrinfo)
        return conn

    return _factory


class _SSRFSafeHTTPHandler(urllib.request.HTTPHandler):
    """Resolves + validates + pins on EVERY request this handler is asked to
    open -- including a redirect target, since `HTTPRedirectHandler` builds a
    brand new `Request` per hop and the `OpenerDirector` re-dispatches it
    through this same `http_open()`, so this closes the post-redirect TOCTOU
    too (the previous per-module code only re-validated the resolved final
    URL's hostname AFTER the redirected connection had already happened)."""

    def __init__(self, context_label):
        super().__init__()
        self._context_label = context_label

    def http_open(self, req):
        hostname = urlsplit(req.full_url).hostname
        validated = resolve_ssrf_safe_addresses(hostname, self._context_label)
        return self.do_open(
            _make_pinned_http_class(http.client.HTTPConnection, validated), req
        )


class _SSRFSafeHTTPSHandler(urllib.request.HTTPSHandler):
    """HTTPS counterpart of `_SSRFSafeHTTPHandler`. `context=self._context`
    (an `ssl.SSLContext`, defaulted the same way `urllib.request.HTTPSHandler`
    itself defaults it -- `ssl.create_default_context()`-equivalent, real
    certificate + hostname verification) is forwarded exactly the way stock
    `HTTPSHandler.https_open()` forwards it, so nothing about TLS
    verification changes -- only which raw address the socket dials."""

    def __init__(self, context_label, context=None, check_hostname=None):
        super().__init__(context=context, check_hostname=check_hostname)
        self._context_label = context_label

    def https_open(self, req):
        hostname = urlsplit(req.full_url).hostname
        validated = resolve_ssrf_safe_addresses(hostname, self._context_label)
        return self.do_open(
            _make_pinned_http_class(http.client.HTTPSConnection, validated),
            req,
            context=self._context,
        )


def _build_ssrf_safe_opener(context_label, https_only, ssl_context=None):
    """Builds a fresh `OpenerDirector` with ONLY the handlers this fetch
    needs -- deliberately NOT `urllib.request.build_opener()`, and deliberately
    no `ProxyHandler`: an opener built by `build_opener()` would end up with
    BOTH a stock, unpinned `HTTPHandler`/`HTTPSHandler` and this module's own
    pinned ones registered for the same scheme, and `OpenerDirector` has no
    guarantee it tries the safe one first; a `ProxyHandler` would mean an
    environment `http_proxy`/`https_proxy` variable is what actually gets
    resolved and validated, not the real target host. Never installed via
    `install_opener()` (process-global mutable state) -- callers use the
    returned instance directly and let it be garbage collected."""
    opener = urllib.request.OpenerDirector()
    opener.add_handler(urllib.request.UnknownHandler())
    opener.add_handler(urllib.request.HTTPDefaultErrorHandler())
    opener.add_handler(urllib.request.HTTPRedirectHandler())
    opener.add_handler(urllib.request.HTTPErrorProcessor())
    if not https_only:
        opener.add_handler(_SSRFSafeHTTPHandler(context_label))
    opener.add_handler(_SSRFSafeHTTPSHandler(context_label, context=ssl_context))
    return opener


# [@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]
def urlopen_ssrf_safe(request, context_label, *, https_only=False, timeout=None, ssl_context=None):
    """Drop-in-shaped replacement for `urllib.request.urlopen()`: same
    context-manager-compatible return value, but resolves + validates +
    pins the connection to a safe address on EVERY hop (the original URL and
    any redirect target) rather than trusting a second, independently-timed
    DNS lookup. Raises `SSRFValidationError` (not `urllib.error.URLError`)
    when a hop fails the safety check.

    `request` may be a `urllib.request.Request` or a plain URL string, same
    as `urlopen()` itself accepts. `https_only=True` omits the plain-HTTP
    handler entirely (for a caller whose own URL-scheme check already
    requires `https://`), so a redirect to `http://` fails with "unknown url
    type" rather than silently being allowed through unpinned. `ssl_context`
    lets a caller (namely this module's own tests, against a self-signed
    local test server) supply a non-default `ssl.SSLContext` -- omitted,
    this is `None`, exactly the same default `urllib.request.HTTPSHandler`
    itself uses."""
    opener = _build_ssrf_safe_opener(context_label, https_only, ssl_context=ssl_context)
    if timeout is None:
        return opener.open(request)
    return opener.open(request, timeout=timeout)
