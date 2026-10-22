<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Rate Limits and Logs See the Real Visitor, Not a Forged Header

As a **Platform Operator**, I want everything that limits or records a visitor by network address (sign-up throttles, event-report spam filters, audit logs) to use the visitor's real address, and I want a visitor who sends their own `CF-Connecting-IP` or `X-Forwarded-For` header to be unable to pick the address that is recorded.

`zero_sudo.security.utils` `_get_trusted_client_ip(request_obj=None)` is the single place that decides this. [@ANCHOR: zero_sudo:get_trusted_client_ip]

* It looks at the transport-level peer of the request (`remote_addr`), never at a header first.
* This deployment reaches Odoo only through a Cloudflare Tunnel whose daemon runs on the same host, so a peer of `127.0.0.1` or `::1` is the tunnel and the headers it injected are genuine. Then the address is `CF-Connecting-IP` if present, otherwise the first entry of `X-Forwarded-For`, otherwise the peer itself.
* Any other peer did not come through the tunnel, so those headers are attacker-controlled and are ignored entirely: the peer address is returned.
* With no request object given it uses the current request, and returns nothing when there is none (a cron job or a shell).
* If a website ever moves from the Tunnel to Cloudflare's classic direct proxy, this single loopback test must become a per-website deployment-mode check; the reasoning is in `docs/bug_hunt_claims/hams_open/cloudflare/models/claims/cf_get_request_context.md`.

Tests: `zero_sudo/tests/test_security_utils.py` (loopback with each header, a non-loopback peer with forged headers, no request).
