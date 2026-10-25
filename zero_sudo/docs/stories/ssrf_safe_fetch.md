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
