<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Server-Side Download Cannot Be Steered at Our Own Network

As a **Platform Operator**, I want any place where the server downloads a URL somebody else supplied to be unable to reach loopback, private-network, link-local or cloud-metadata addresses, even when the attacker controls the DNS for the host name. As a **Developer**, I want one plain, Odoo-free helper to call instead of `urllib.request.urlopen()`, so I do not have to re-derive the checks.

The helper is `zero_sudo/daemon/ssrf_safe_fetch.py`. It lives in `zero_sudo` because it is a common ancestor of its callers (`binary_downloader` and the `pager_duty` spooler daemon), and it imports nothing from `odoo`, so a standalone systemd daemon can load it.

## The Problem

A caller used to check a URL by resolving its host name once and rejecting unsafe addresses, then pass the same host name to `urlopen()`, which resolved it a second time to connect. An attacker who controls the name's DNS answers the first lookup with a public address and the second with an internal one. A redirect has the same weakness at each hop. The fix is to resolve and validate once per hop and make the connection dial exactly the addresses that were validated.

## What a Safe Address Is `[@ANCHOR: zero_sudo:ssrf_safe_fetch_is_ssrf_safe_public_ip]`

`is_ssrf_safe_public_ip(ip_obj)` is true only for a globally routable address. It also names each excluded range explicitly (private, loopback, link-local, which covers the `169.254.169.254` metadata address, multicast, reserved and unspecified), so the rule stays legible and does not widen if a later Python release changes what `is_global` means.

## Resolve Once, Fail Closed `[@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]`

`resolve_ssrf_safe_addresses(hostname, context)` makes exactly one `getaddrinfo()` call. It raises `SSRFValidationError` when the host name is empty, when it cannot be resolved or resolves to nothing, or when any one of the returned addresses is unsafe, because a real connection could pick any of them. On success it returns the full address list so the caller can pin the connection to it. `context` is a short label folded into the error message.

## Dial Only What Was Validated `[@ANCHOR: zero_sudo:ssrf_safe_fetch_pinned_create_connection]`

`_pinned_create_connection(validated_addrinfo)` returns a replacement for `socket.create_connection` that never resolves anything itself. It tries the validated stream-socket addresses in order and raises the last connection error if all of them fail. Only the requested port is taken from the connection's address; the host part is ignored, which is what keeps a second DNS lookup from happening. Datagram and raw entries that `getaddrinfo()` returns for the same address are skipped, because a UDP `connect()` always appears to succeed and would hand `http.client` a socket that hangs. The function is installed on an `HTTPConnection` or `HTTPSConnection` through its `_create_connection` attribute, which leaves the `Host` header, TLS SNI and certificate host-name verification exactly as an ordinary request has them.

## The Drop-In `[@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]`

`urlopen_ssrf_safe(request, context_label, *, https_only=False, timeout=None, ssl_context=None)` is used like `urlopen()`: it accepts a `Request` or a URL string and returns a context-manager response. Its opener resolves, validates and pins on every hop, including each redirect target, and raises `SSRFValidationError` instead of `URLError` when a hop fails the check. With `https_only=True` the plain-HTTP handler is left out, so a redirect to `http://` fails rather than passing through unpinned. `ssl_context` lets a test supply a context that trusts a self-signed local server.


## The Pieces Behind the Drop-In

These are internal, but each is a real decision point in the fetch, so each is described here.

### The Opener `[@ANCHOR: zero_sudo:ssrf_safe_fetch_build_opener]`

- **Purpose:** `_build_ssrf_safe_opener(context_label, https_only, ssl_context=None, follow_redirects=True)` builds the one `urllib` opener that `urlopen_ssrf_safe()` uses for a single call. Nothing else builds one.
- **Call interface:** `context_label` is the short label folded into error messages. `https_only` (bool): when true the plain-HTTP handler is not added. `ssl_context` is an `ssl.SSLContext` or `None` (the stock default, with certificate and host-name checking). `follow_redirects` (bool): true uses the stock redirect handler, false uses the one that refuses every redirect so the 3xx answer reaches the caller as an `HTTPError`. Returns a new `OpenerDirector`; raises nothing.
- **Special concerns:** it never calls `urllib.request.build_opener()` and adds no proxy handler. A stock opener would register an unpinned HTTP(S) handler beside ours, and `OpenerDirector` does not promise to try ours first; a proxy handler would make an environment `http_proxy` the host that is resolved and checked. It is never installed with `install_opener()`, so there is no process-global state; the caller uses the instance and lets it go.
- **See also:** `[@ANCHOR: zero_sudo:ssrf_safe_fetch_urlopen_ssrf_safe]`, `[@ANCHOR: zero_sudo:ssrf_safe_fetch_no_redirect]`. **Tests:** `test_urlopen_ssrf_safe_pins_the_connection_and_sets_the_real_host_header_and_sni`, `test_a_refused_redirect_reaches_the_caller_as_an_http_error`, `test_http_open_pins_a_plain_http_request_and_checks_the_address_first`.

### The Plain-HTTP Handler `[@ANCHOR: zero_sudo:ssrf_safe_fetch_http_handler_init]` `[@ANCHOR: zero_sudo:ssrf_safe_fetch_http_open]`

- **Purpose:** `_SSRFSafeHTTPHandler` is the opener's `http://` handler. Its `__init__(context_label)` keeps the label for error messages. Its `http_open(req)` resolves the request's host name once, refuses any non-public address, then sends the request over a connection pinned to the addresses it just validated.
- **Call interface:** `http_open(req)` takes a `urllib.request.Request` and returns the response; the opener calls it, nobody else should. It raises `SSRFValidationError` (missing host name, unresolvable, or any unsafe address) before any connection is made.
- **Special concerns:** it runs for every request the opener dispatches, and a redirect is a new request, so every hop is re-resolved and re-checked. It makes one DNS lookup and no second one for the connection. It performs the network request.
- **`__init__` and the helpers:** `__init__(context_label)` takes only the label, calls the base handler's constructor and returns nothing; it does no network work. `http_open` gets the validated addresses from `resolve_ssrf_safe_addresses(hostname, context)` (anchor `zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses`), which makes exactly one `getaddrinfo` call and raises `SSRFValidationError` for an empty host name, a failed or empty lookup, or any returned address that is not public, so a name that answers with one public and one private address is refused as a whole; on success it returns the full address list. The connection factory comes from `_make_pinned_http_class(base_cls, validated_addrinfo)` (anchor `zero_sudo:ssrf_safe_fetch_make_pinned_http_class`): it returns a callable that builds a `base_cls` connection (`HTTPConnection`) with the host passed through unchanged, so the `Host` header keeps the name the caller wrote, and swaps in a connect step that dials only the validated stream addresses. Errors from the resolver reach the caller of `http_open` unchanged.
- **See also:** `[@ANCHOR: zero_sudo:ssrf_safe_fetch_resolve_ssrf_safe_addresses]`, `[@ANCHOR: zero_sudo:ssrf_safe_fetch_make_pinned_http_class]`. **Tests:** `test_the_handlers_keep_their_context_label_and_the_https_handler_its_ssl_context`, `test_http_open_pins_a_plain_http_request_and_checks_the_address_first`.

### The HTTPS Handler `[@ANCHOR: zero_sudo:ssrf_safe_fetch_https_handler_init]` `[@ANCHOR: zero_sudo:ssrf_safe_fetch_https_open]`

- **Purpose:** `_SSRFSafeHTTPSHandler` is the same thing for `https://`. Its `__init__(context_label, context=None, check_hostname=None)` keeps the label and passes `context` and `check_hostname` to the stock `HTTPSHandler`, so TLS verification is unchanged. Its `https_open(req)` resolves, validates and pins exactly as the plain handler does, then hands the stock `do_open` the same `ssl_context` the stock handler would use.
- **Call interface:** as `http_open`; additionally the TLS handshake can raise `ssl.SSLError` or `URLError` wrapping it.
- **Special concerns:** only the raw address the socket dials changes. The `Host` header, TLS SNI and certificate host-name check use the original host name, so a certificate for a different name still fails.
- **See also:** `[@ANCHOR: zero_sudo:ssrf_safe_fetch_make_pinned_http_class]`. **Tests:** `test_urlopen_ssrf_safe_pins_the_connection_and_sets_the_real_host_header_and_sni`, `test_urlopen_ssrf_safe_still_performs_real_tls_hostname_verification`, `test_the_handlers_keep_their_context_label_and_the_https_handler_its_ssl_context`.

### The Pinned Connection Class `[@ANCHOR: zero_sudo:ssrf_safe_fetch_make_pinned_http_class]`

- **Purpose:** `_make_pinned_http_class(base_cls, validated_addrinfo)` returns the factory the handlers give to `do_open` in place of a connection class.
- **Call interface:** `base_cls` is `http.client.HTTPConnection` or `HTTPSConnection`; `validated_addrinfo` is the list `resolve_ssrf_safe_addresses()` returned. The returned factory is called as `factory(host, timeout=..., **kwargs)` and returns a real `base_cls` instance whose `_create_connection` is the pinned dialer.
- **Special concerns:** `host` and `kwargs` go to `base_cls` unchanged, so `self.host` and `self.port` (Host header, SNI, certificate check) are what an ordinary connection has. Each call builds a fresh closure; nothing is shared between requests.
- **See also:** `[@ANCHOR: zero_sudo:ssrf_safe_fetch_pinned_create_connection]`. **Tests:** `test_make_pinned_http_class_builds_a_connection_that_only_dials_the_validated_address`, `test_urlopen_ssrf_safe_pins_the_connection_and_sets_the_real_host_header_and_sni`.
