<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: The External RPC Endpoints Answer Only the Machine They Run On

As the **Operator** of an Odoo server, I want XML-RPC, JSON-RPC and JSON-2 to refuse any caller that is not on the
server itself, without my writing a firewall rule, so that a stolen password or API key cannot be used from the
Internet and a scanner finds no endpoint at all.

1. A daemon on the Odoo host calls `127.0.0.1` with no forwarding header and is answered as before.
   `[@ANCHOR: hams_rpc_guard:origin_is_local]`
2. A visitor's request arrives through nginx or Cloudflare Tunnel: the peer is loopback, but the proxy's headers are
   present, so `/xmlrpc/...` and `/jsonrpc` answer 404. `[@ANCHOR: hams_rpc_guard:endpoint_gate]`
3. `/json/2/...` stays open by default so a daemon on another machine keeps working; the operator closes it with
   `hams_rpc_guard.json2_access`. `[@ANCHOR: hams_rpc_guard:policy_allows]`
4. The web client's `call_kw` and `call_button` are unaffected, and the public visitor still cannot use them.
   `[@ANCHOR: hams_rpc_guard:endpoint_family]`

5. Installing the module adds its end-user documentation article and sets no system parameter, so an operator's
   explicit choice is never overwritten. `[@ANCHOR: hams_rpc_guard:post_init_hook]`

## Helpers

- `[@ANCHOR: hams_rpc_guard:refuse_endpoint_when_closed]` reads the policy and the trusted proxies, judges the request,
  counts and refuses (404) when the policy says no.
- `[@ANCHOR: hams_rpc_guard:parse_address]` turns one header token (plain, `host:port`, `[v6]:port`, IPv4-mapped) into an
  address, or nothing when it is not one. `[@ANCHOR: hams_rpc_guard:client_addresses]` gathers every client address the
  forwarding headers carry; an unparseable one is kept as "nothing" so it can never pass for loopback.
- `[@ANCHOR: hams_rpc_guard:parse_trusted_proxies]` reads the comma-separated trusted-proxy parameter and drops junk entries.
- `[@ANCHOR: hams_rpc_guard:normalise_policy]` maps the stored parameter to a policy: unset takes the default, junk
  fails closed to `loopback`.
- `[@ANCHOR: hams_rpc_guard:note_denial]` counts a refusal per endpoint family and logs at the first and every power of ten
  (`[@ANCHOR: hams_rpc_guard:is_power_of_ten]`), family and count only; `[@ANCHOR: hams_rpc_guard:denial_counts]` returns a
  copy of the counts.

## Verification

`hams_rpc_guard/tests/test_endpoint_gate.py` makes real HTTP requests with and without forwarding headers under every
policy value and checks the answers (200, 401 or 404) and that the log carries no request value.
