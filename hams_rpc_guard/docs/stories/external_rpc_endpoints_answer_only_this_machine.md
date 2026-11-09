<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: The External RPC Endpoints Answer Only Allowed Networks, or Only a Unix Socket

As the **Operator** of an Odoo server, I want XML-RPC, JSON-RPC and JSON-2 to refuse any caller that is not on a
network I allowed (by default: the server itself), without my writing a firewall rule, so that a stolen password or API
key cannot be used from the Internet and a scanner finds no endpoint at all; and I want my own daemons on the server to
be identified by the kernel rather than only by a key.

1. A daemon on the Odoo host calls `127.0.0.1` with no forwarding header and is answered as before: the default
   `hams_rpc_guard.allowed_networks` is loopback. `[@ANCHOR: hams_rpc_guard:resolve_client]`
   `[@ANCHOR: hams_rpc_guard:judge_tcp]`
2. A visitor's request arrives through nginx or Cloudflare Tunnel: the peer is loopback, but the proxy's headers are
   present and the proxy is not trusted, so every endpoint answers 404, whatever addresses the visitor forges.
   `[@ANCHOR: hams_rpc_guard:endpoint_gate]`
3. Daemons on a private network (WireGuard) come in through a reverse proxy I trust (`trusted_proxies`), which sets one
   header (`client_address_header`) to their address; that network is on `allowed_networks`.
   `[@ANCHOR: hams_rpc_guard:gate_settings]`
4. A typo in a network list is refused when I save it, and if a bad value reaches the database anyway the gate logs an
   error and treats it as closed. `[@ANCHOR: hams_rpc_guard:check_gate_parameters]` `[@ANCHOR: hams_rpc_guard:read_settings]`
5. With `rpc_transport = unix` the endpoints are refused on TCP and served only on a Unix socket; each connecting
   process's uid/gid (`SO_PEERCRED`) must be in `unix_peers` and may act only as the Odoo logins it is bound to.
   `[@ANCHOR: hams_rpc_guard:judge_unix]` `[@ANCHOR: hams_rpc_guard:install_server_patches]`
   `[@ANCHOR: hams_rpc_guard:unix_identity]`
6. With `hams_rpc_guard.mode = report` nothing is refused: would-be refusals are counted by network class and logged.
   `[@ANCHOR: hams_rpc_guard:note_request]`
7. The web client's `call_kw` and `call_button` are unaffected (and stay on TCP), and the public visitor still cannot
   use them. `[@ANCHOR: hams_rpc_guard:endpoint_family]`
8. Installing the module adds its end-user documentation article and sets no system parameter, so an operator's
   explicit choice is never overwritten. `[@ANCHOR: hams_rpc_guard:post_init_hook]`

## Helpers

- `[@ANCHOR: hams_rpc_guard:refuse_endpoint_when_closed]` reads the mode and settings, judges the request, counts and
  refuses (404) unless report mode.
- `[@ANCHOR: hams_rpc_guard:gate_parameters]` the parameter names, words and environ keys.
- `[@ANCHOR: hams_rpc_guard:parse_address]`, `[@ANCHOR: hams_rpc_guard:socket_peer]`,
  `[@ANCHOR: hams_rpc_guard:network_class]` turn a peer or header into an address and an address into a counter class.
- `[@ANCHOR: hams_rpc_guard:parse_networks]`, `[@ANCHOR: hams_rpc_guard:parse_trusted_proxies]`,
  `[@ANCHOR: hams_rpc_guard:parse_client_address_header]`, `[@ANCHOR: hams_rpc_guard:parse_legacy_access]`,
  `[@ANCHOR: hams_rpc_guard:parse_transport]`, `[@ANCHOR: hams_rpc_guard:parse_peer_rules]` parse the settings;
  `[@ANCHOR: hams_rpc_guard:parameter_parsers]` maps each setting to its parser.
- `[@ANCHOR: hams_rpc_guard:peer]`, `[@ANCHOR: hams_rpc_guard:peer_rule]`, `[@ANCHOR: hams_rpc_guard:peer_rules]` are the
  socket identity and its rules.
- `[@ANCHOR: hams_rpc_guard:request_counts]` and `[@ANCHOR: hams_rpc_guard:is_power_of_ten]` are the counters.
- The listener: `[@ANCHOR: hams_rpc_guard:listener_options]`, `[@ANCHOR: hams_rpc_guard:listener_config]`,
  `[@ANCHOR: hams_rpc_guard:open_listener]`, `[@ANCHOR: hams_rpc_guard:peer_of]`, `[@ANCHOR: hams_rpc_guard:rpc_only_app]`,
  `[@ANCHOR: hams_rpc_guard:unix_request_handler]`, `[@ANCHOR: hams_rpc_guard:unix_wsgi_server]`,
  `[@ANCHOR: hams_rpc_guard:serve_unix_connection]`, `[@ANCHOR: hams_rpc_guard:listener_socket]`,
  `[@ANCHOR: hams_rpc_guard:ensure_listener]`.
- The daemon client: `[@ANCHOR: zero_sudo:odoo_rpc_client]`, `[@ANCHOR: zero_sudo:odoo_rpc_client_call]`,
  `[@ANCHOR: zero_sudo:unix_http_connection]`.

## Verification

`tests/test_endpoint_gate.py` makes real HTTP requests with and without forwarding headers (forged ones from a peer
that is not a trusted proxy included) under the settings and checks the answers (200, 401 or 404), the fail-closed
reading and the write-time validation, and that the log carries no request value. `tests/test_unix_listener.py` runs
the listener on a scratch socket in the test process and calls it with the daemon client. `tools/prefork_unix_check.py`
runs a throwaway Odoo with `--workers 2` (scratch database cluster, port and socket) and checks the prefork path.
