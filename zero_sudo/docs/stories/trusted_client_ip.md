<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Rate Limits and Logs See the Real Visitor, Not a Forged Header

As a **Platform Operator**, I want everything that limits or records a visitor by network address (sign-up throttles, event-report spam filters, audit logs) to use the visitor's real address, and I want a visitor who sends their own `CF-Connecting-IP` or `X-Forwarded-For` header to be unable to pick the address that is recorded.

`zero_sudo.security.utils` `_get_trusted_client_ip(request_obj=None)` is the single place that decides this.

## `_get_trusted_client_ip` `[@ANCHOR: zero_sudo:get_trusted_client_ip]`

**Purpose.** The one decision about which network address a rate limit or a log row uses for a visitor.

**Call interface.** `@api.model`; `env["zero_sudo.security.utils"]._get_trusted_client_ip(request_obj=None)`

| Argument | Type | Meaning |
|---|---|---|
| `request_obj` | request or `None` | an already-unwrapped Odoo request (tests, or a caller that has one); `None` uses the current request |

Returns the client address as a `str`, or `None` when no request is active (a cron job or a shell). Raises nothing of its own.

**How it decides.**

* It looks at the transport-level peer of the request (`remote_addr`), never at a header first.
* This deployment reaches Odoo only through a Cloudflare Tunnel whose daemon runs on the same host, so a loopback peer (`127.0.0.1`, `::1`, or the IPv4-mapped `::ffff:127.0.0.1` that a dual-stack listener reports) is the tunnel and the headers it injected are genuine. Then the address is `CF-Connecting-IP` if present, otherwise the first entry of `X-Forwarded-For`, otherwise the peer itself.
* Any other peer did not come through the tunnel, so those headers are attacker-controlled and are ignored entirely: the peer address is returned.
* With no request object given it uses the current request, and returns nothing when there is none (a cron job or a shell).

**Special concerns.** Security: the value keys sign-up throttles, spam filters and the audit log, so a visitor who sends their own `CF-Connecting-IP` or `X-Forwarded-For` header must not be able to choose it; the loopback test is what prevents that. The returned text is not validated as an IP literal, so a caller that needs an address object passes it through `parse_peer_address`. It reads the request only: no database access and no side effect. If a website ever moves from the Tunnel to Cloudflare's classic direct proxy, this single loopback test must become a per-website deployment-mode check; the reasoning is in `docs/bug_hunt_claims/hams_open/cloudflare/models/claims/cf_get_request_context.md`.

**See also.** `is_loopback_peer`, `parse_peer_address`; the web-login block in `controllers/main.py`, which records this value in the security log; tests below.

## `parse_peer_address` [@ANCHOR: zero_sudo:parse_peer_address]

**Purpose.** Turns a transport peer address (`REMOTE_ADDR`) into an address object that can be compared, with an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`, what a dual-stack `[::]` listener reports for an IPv4 peer) returned as the IPv4 address `a.b.c.d`. Used by `is_loopback_peer` and by the Cloudflare peer tests (`cloudflare/wsgi_proxy_scheme.py`, `cloudflare/models/trusted_ip_ranges.py`) so that no code compares the text of a peer address with a literal.

**Call interface.** `zero_sudo.peer_address.parse_peer_address(address)`

| Argument | Type | Meaning |
|---|---|---|
| `address` | `str` or `None` | The peer address as the server reports it. Surrounding white space is ignored. Not modified or retained. |

Returns an `ipaddress.IPv4Address` or `ipaddress.IPv6Address`, or `None` when `address` is missing, empty, a host name or not an IP literal. Raises nothing.

**Special concerns.** Pure: no database, network or state; safe in the WSGI entry hook before any request exists. A scoped IPv6 address (`fe80::1%eth0`) is accepted by Python 3.9+; its scope is kept and it is never loopback.

**See also.** `is_loopback_peer`; tests `test_28b2_get_trusted_client_ip_trusts_cf_header_from_ipv4_mapped_loopback`, `test_28b3_peer_address_helpers`.

## `is_loopback_peer` [@ANCHOR: zero_sudo:is_loopback_peer]

**Purpose.** The "this request came through the local Cloudflare Tunnel" test: true for a loopback peer. It replaces the literal comparison with `127.0.0.1` / `::1`, which failed for `::ffff:127.0.0.1`, so every visitor looked like one client behind a dual-stack listener.

**Call interface.** `zero_sudo.peer_address.is_loopback_peer(address)`

| Argument | Type | Meaning |
|---|---|---|
| `address` | `str` or `None` | The peer address (see `parse_peer_address`). |

Returns `True` for an address in 127.0.0.0/8, for `::1` and for the mapped form of a 127.0.0.0/8 address; `False` for everything else, including text that is not an IP address. Raises nothing.

**Special concerns.** Pure and cheap. Security: a `True` result makes `CF-Connecting-IP` and `X-Forwarded-For` trusted, so it must only be given the transport peer, never a header value.

**See also.** `parse_peer_address`, `_get_trusted_client_ip`; the tests above.

Tests: `zero_sudo/tests/test_security_utils.py` (loopback with each header, a non-loopback peer with forged headers, no request, the mapped forms).
