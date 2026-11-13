<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: The External RPC Endpoints Answer Only Allowed Networks, or Only a Unix Socket

As the **Operator** of an Odoo server, I want XML-RPC, JSON-RPC and JSON-2 to refuse any caller that is not on a
network I allowed (by default: the server itself), without my writing a firewall rule, so that a stolen password or API
key cannot be used from the Internet and a scanner finds no endpoint at all; and I want my own daemons on the server to
be identified by the kernel rather than only by a key.



The story, step by step. Each function that makes it true has its own entry below, grouped by source file.

1. A daemon on the Odoo host calls `127.0.0.1` with no forwarding header and is answered as before: the default
   `hams_rpc_guard.allowed_networks` is loopback (see `resolve_client` and `judge_tcp`).
2. A visitor's request arrives through nginx or Cloudflare Tunnel: the peer is loopback, but the proxy's headers are
   present and the proxy is not trusted, so every endpoint answers 404, whatever addresses the visitor forges.
3. Daemons on a private network (WireGuard) come in through a reverse proxy I trust (`trusted_proxies`), which sets one
   header (`client_address_header`) to their address; that network is on `allowed_networks`.
4. A typo in a network list is refused when I save it, and if a bad value reaches the database anyway the gate logs an
   error and treats it as closed. A saved change reaches every running worker at once.
5. With `rpc_transport = unix` the endpoints are refused on TCP and served only on a Unix socket; each connecting
   process's uid/gid (`SO_PEERCRED`) must be in `unix_peers` and may act only as the Odoo logins it is bound to.
6. With `hams_rpc_guard.mode = report` nothing is refused: would-be refusals are counted by network class and logged.
7. The web client's `call_kw` and `call_button` are unaffected (and stay on TCP), and the public visitor still cannot
   use them.
8. Installing the module adds its end-user documentation article and sets no system parameter, so an operator's
   explicit choice is never overwritten.

## `hams_rpc_guard/endpoint_gate.py`

### Parameter names and words: `ALLOWED_NETWORKS_PARAM` and the rest [@ANCHOR: hams_rpc_guard:gate_parameters]

The module-level constants every other function uses: the six system parameter names (`hams_rpc_guard.allowed_networks`, `legacy_access`, `trusted_proxies`, `client_address_header`, `rpc_transport`, `unix_peers`), their words (`none`, `networks`, `tcp`, `unix`, `both`, and `closed`, which a junk transport is read as), the default `127.0.0.0/8,::1/128`, the single-address headers a proxy may set (`CF-Connecting-IP`, `X-Real-IP`, `True-Client-IP`), the forwarding header list, the counter classes, and the two WSGI environ keys. Constants only: no I/O, no lock. `X-Forwarded-For` is deliberately not a client-address header, because the client starts that list.

### `endpoint_family(path) -> str | None` [@ANCHOR: hams_rpc_guard:endpoint_family]

Classifies a request path. Argument `path`: the request path (a `str`; anything else gives None). Returns `legacy-rpc` for `/jsonrpc` and anything under `/xmlrpc/`, `json2` for `/json/2` and anything under `/json/2/`, otherwise None (not gated). The separator matters: `/xmlrpc.php`, `/jsonrpc2` and `/json/20` are look-alikes and return None. Pure, never raises. Security: the web client's `/web/dataset/call_kw` is deliberately not a family, so it is never gated here. Tests: `tests/test_endpoint_gate.py` `TestEndpointFamily`.

### `_parse_address(text) -> ipaddress object | None` [@ANCHOR: hams_rpc_guard:parse_address]

Turns a peer or header value into an `ipaddress` address. Argument `text`: any value, converted with `str`, trimmed and unquoted. Accepts a bracketed IPv6 (`[::1]`), a `host:port` with exactly one colon (the port is dropped) and a zone suffix (`%eth0`, dropped). An IPv4-mapped IPv6 address becomes the IPv4 address, so a mapped loopback is judged as loopback. Returns None when the text is not an address; never raises. Pure. Tests: `TestResolveClient` in `tests/test_endpoint_gate.py`.

### `parse_networks(value, param=ALLOWED_NETWORKS_PARAM, default="") -> tuple` [@ANCHOR: hams_rpc_guard:parse_networks]

Parses a comma-separated network setting. Arguments: `value` (the stored text; None or False reads as blank), `param` (the parameter name used in the error message), `default` (taken when the value is blank). Returns a tuple of `ipaddress` networks, lenient about host bits (`strict=False`; a bare host is a /32 or /128); the word `none` gives the empty tuple, meaning no network. Raises `ConfigError` naming `param` and the bad item for anything that is not an address or CIDR. Pure. Tests: `test_networks_and_hosts_parse`, `test_a_bad_cidr_is_a_loud_error_never_an_open_gate`.

### `parse_trusted_proxies(value) -> tuple` [@ANCHOR: hams_rpc_guard:parse_trusted_proxies]

`parse_networks` for `hams_rpc_guard.trusted_proxies`, with the default empty (no proxy is trusted). Argument `value`: the stored text. Returns the tuple of networks of reverse proxies whose client-address header may be believed. Raises `ConfigError` on junk. Pure. Security: an empty list means every request that carries a forwarding header is refused, which is the safe reading when the operator has configured nothing. Tests: `TestResolveClient`, `test_a_trusted_proxy_names_the_client_in_the_one_configured_header`.

### `parse_client_address_header(value) -> str` [@ANCHOR: hams_rpc_guard:parse_client_address_header]

Validates the header a trusted proxy sets to the true client address. Argument `value`: the stored text. Returns the canonical spelling of one of `CF-Connecting-IP`, `X-Real-IP` or `True-Client-IP` (matched case-insensitively), or "" when unset. Raises `ConfigError` for anything else, `X-Forwarded-For` included, because that header is a list whose first entries the client writes. Pure. Security: restricting the choice to single-address headers a proxy overwrites is what stops a client choosing its own address. Tests: `test_only_single_address_headers_can_be_trusted`.

### `parse_legacy_access(value) -> str` [@ANCHOR: hams_rpc_guard:parse_legacy_access]

Parses `hams_rpc_guard.legacy_access`, which governs `/xmlrpc/...` and `/jsonrpc` only. Argument `value`: the stored text. Returns `networks` (unset, or the word: the legacy endpoints follow `allowed_networks`) or `none` (closed to everyone, even on an allowed network). Raises `ConfigError` on anything else. When read by `read_settings`, junk is turned into `none`. Pure. Tests: `test_policy_words`, `test_legacy_switch_and_transport`.

### `parse_transport(value) -> str` [@ANCHOR: hams_rpc_guard:parse_transport]

Parses `hams_rpc_guard.rpc_transport`: which transport may carry the endpoints. Argument `value`: the stored text. Returns `tcp` (unset, the default), `unix` (refused on TCP, served only on the Unix socket) or `both`. Raises `ConfigError` on anything else. `read_settings` reads junk as `closed`, which no transport satisfies. Pure. Tests: `test_policy_words`, `test_unix_transport_refuses_every_endpoint_on_tcp`.

### `class Peer` [@ANCHOR: hams_rpc_guard:peer]

The verified credentials of the process at the far end of a Unix-socket connection. Slots: `pid`, `uid`, `gid` (ints, taken by the kernel from `SO_PEERCRED` at `connect()`, so the peer cannot lie), `user` and `group` (the names for the uid and gid, or "" when they have none). A plain value object: no I/O, no lock. Built by `unix_listener.peer_of`, carried in the WSGI environ as a dict, rebuilt by the gate and matched against `PeerRules`.

### `Peer.__init__(self, pid, uid, gid, user="", group="")` [@ANCHOR: hams_rpc_guard:peer_init]

Constructor of `Peer`. Arguments: `pid`, `uid`, `gid` (ints) and the optional `user` and `group` names, which default to "". It only stores the five values in the slots; it validates nothing, does no lookup and cannot fail. The gate calls it as `Peer(**dict)` on the dictionary that `as_dict` produced, so the keyword names are part of the contract. Tests: `tests/test_unix_listener.py` (peer matching and `test_socket_permissions_and_peer_credentials`).

### `Peer.as_dict(self) -> dict` [@ANCHOR: hams_rpc_guard:peer_as_dict]

Returns the five slots as a dictionary `{pid, uid, gid, user, group}`. Its caller is `UnixRequestHandler.make_environ`, which stores the dictionary in the WSGI environ under `hams_rpc_guard.unix_peer`; a dictionary survives werkzeug's environ handling where an object would be copied or stringified. Pure, no I/O, cannot fail. An HTTP client cannot write that environ key, because werkzeug puts every request header under an `HTTP_` key.

### `class PeerRule` [@ANCHOR: hams_rpc_guard:peer_rule]

One entry of `hams_rpc_guard.unix_peers`. Slots: `kind` (`user`, `uid`, `group` or `gid`), `value` (the name, or the int for uid and gid) and `logins` (a frozenset of Odoo logins the matching peer may authenticate as; empty means any). Built by `parse_peer_rules`; a value object with no I/O and no lock. The `group` and `gid` kinds compare the peer's primary group only, since `SO_PEERCRED` carries no supplementary groups.

### `PeerRule.__init__(self, kind, value, logins=frozenset())` [@ANCHOR: hams_rpc_guard:peer_rule_init]

Constructor of `PeerRule`. Arguments: `kind` (one of `user`, `uid`, `group`, `gid`), `value` (str or int to compare with) and `logins` (any iterable of login names, converted to a frozenset; default empty meaning the peer may act as any login). Stores the three values; no validation here, because `parse_peer_rules` has already checked the text. Cannot fail on valid input. Tests: `tests/test_unix_listener.py` `test_entries`.

### `PeerRule.matches(self, peer) -> bool` [@ANCHOR: hams_rpc_guard:peer_rule_matches]

True when `peer` (a `Peer`) has the attribute this rule names (`user`, `uid`, `group` or `gid`) equal to the rule's `value`. Exact equality, case-sensitive for names; no pattern matching. Pure, no I/O. Security: because the uid and gid come from the kernel, a rule on a numeric uid cannot be spoofed by a caller; a name rule is only as good as the passwd database's mapping from that uid. Tests: `test_matching_takes_the_first_rule`.

### `class PeerRules` [@ANCHOR: hams_rpc_guard:peer_rules]

The parsed `hams_rpc_guard.unix_peers`: an ordered tuple of `PeerRule`. `match(peer)` returns the first rule that matches or None, and None means the peer is refused. An empty tuple (setting unset or blank) therefore refuses every peer: opening the socket admits no one until the operator lists who may use it. Immutable after construction, no I/O, no lock. Tests: `tests/test_unix_listener.py` `test_matching_takes_the_first_rule`, `test_judge_unix`.

### `PeerRules.__init__(self, rules=())` [@ANCHOR: hams_rpc_guard:peer_rules_init]

Constructor of `PeerRules`. Argument `rules`: any iterable of `PeerRule`, kept in the given order as a tuple (order decides which rule wins when two match). Default is no rules, which refuses every peer. No validation, no I/O. `parse_peer_rules` is the normal caller; the gate holds the instance inside `GateSettings.unix_peers`.

### `PeerRules.match(self, peer) -> PeerRule | None` [@ANCHOR: hams_rpc_guard:peer_rules_match]

Returns the first `PeerRule` whose `matches(peer)` is true, or None when none does. The first match wins, so an earlier narrow rule (for example `daemon=login`) can restrict a peer before a later broad one. Argument `peer`: a `Peer`. Pure, no I/O, cannot fail. `judge_unix` uses the returned rule's `logins` as the set of Odoo logins the caller may act as, and treats None as a refusal with reason `peer`. Tests: `test_matching_takes_the_first_rule`.

### `parse_peer_rules(value) -> PeerRules` [@ANCHOR: hams_rpc_guard:parse_peer_rules]

Parses `hams_rpc_guard.unix_peers`. Argument `value`: the stored text. Entries are comma-separated `WHO[=LOGIN|LOGIN...]`, where WHO is a user name, a numeric uid, `@group` or `@<gid>`; the optional login list restricts which Odoo accounts that peer may use. Unset or blank gives no rules (everyone refused). Raises `ConfigError` for a malformed entry, an empty login list, or a name that is not a valid user or group word; ASCII digits only count as numeric. Pure. Tests: `test_entries`, `test_malformed_entries_are_errors`.

### `class GateSettings` [@ANCHOR: hams_rpc_guard:gate_settings]

The parsed gate settings of one database as `read_settings` builds them: `allowed_networks` and `trusted_proxies` (tuples of networks), `client_address_header` (str, "" for none), `legacy` (`networks` or `none`), `transport` (`tcp`, `unix`, `both` or `closed`) and `unix_peers` (a `PeerRules`, or None when the setting could not be read, in which case no Unix peer is admitted). A value object with no I/O and no lock, rebuilt for each request so a changed parameter takes effect at once.

### `GateSettings.__init__(self, allowed_networks, trusted_proxies=(), client_address_header="", legacy="networks", transport="tcp", unix_peers=None)` [@ANCHOR: hams_rpc_guard:gate_settings_init]

Constructor of `GateSettings`. `allowed_networks` and `trusted_proxies` are converted to tuples; the other arguments are stored as given. The defaults are the open-to-allowed-list reading (legacy `networks`, transport `tcp`), but the fail-closed caller in `ir_http.py` passes `legacy=none`, `transport=closed` and no networks when the parameters cannot be read at all. No validation or I/O. Tests: `test_endpoint_gate.py` (`test_an_unreadable_parameter_fails_closed`).

### `read_settings(get_param) -> GateSettings` [@ANCHOR: hams_rpc_guard:read_settings]

Builds the gate settings from system parameters. Argument `get_param(key, default)`: a reader, in production `zero_sudo.security.utils._get_system_param`. Each setting is parsed on its own; one that raises `ConfigError` is logged at ERROR (parameter name and reason) and replaced by its closed value: no network, no trusted proxy, legacy `none`, transport `closed`, no Unix peer. An unset `allowed_networks` takes the loopback default. Never raises for a bad value; a reader that raises propagates and the caller fails closed. Tests: `test_read_settings_fails_closed_on_each_bad_value_and_says_so`, `test_read_settings_defaults`.

### `socket_peer(environ) -> address | None` [@ANCHOR: hams_rpc_guard:socket_peer]

The address of the process that opened the TCP connection. Argument `environ`: the WSGI environment. It prefers `werkzeug.proxy_fix.orig["REMOTE_ADDR"]` over `REMOTE_ADDR`, because Odoo's `proxy_mode` rewrites `REMOTE_ADDR` from a header the client controls. Returns None when there is no peer or it is not an IP address. Pure. Security: this is the only trustworthy part of a TCP request's origin, so the whole forwarded-header decision starts here. Tests: `TestResolveClient` (`test_proxy_fix_cannot_launder_the_peer`).

### `resolve_client(environ, headers, trusted_proxies=(), client_address_header="") -> address | None` [@ANCHOR: hams_rpc_guard:resolve_client]

Decides who the true client of a TCP request is. Arguments: the WSGI `environ`, the request `headers` (case-insensitive, with `getlist`), a tuple of trusted proxy networks and the canonical header name or "". Rules: no peer gives None; no forwarding header at all gives the peer; a forwarding header with a peer that is not a trusted proxy (or no configured header) gives None; otherwise the single address in the configured header, or None if it is missing, repeated, comma-joined or not an address. Pure, no I/O. Security: forging `X-Forwarded-For: 127.0.0.1` never helps. Tests: `TestResolveClient`.

### `_in_any(address, networks) -> bool` [@ANCHOR: hams_rpc_guard:in_any]

The CIDR membership test: True when `address` lies inside any network in `networks`. Only networks of the same IP version are compared, so an IPv4 address never matches an IPv6 network and no `TypeError` from mixed versions can turn into an exception or an accidental match. An empty tuple gives False, which is how "no network" refuses everyone. Pure, no I/O, no lock. Tests: `TestResolveClient` and `test_allowed_and_denied_networks`.

### `network_class(address) -> str` [@ANCHOR: hams_rpc_guard:network_class]

Maps a client address to the label used in counters and the log: `loopback`, `link-local`, `private`, `public`, or `unknown` for None. Checked in that order, so a loopback address is never counted as private. Argument `address`: an `ipaddress` object or None. Pure. The class is the only thing about the caller that reaches the log, which is why logs never hold an address. Tests: `test_network_classes`.

### `judge_tcp(family, settings, client) -> (bool, str)` [@ANCHOR: hams_rpc_guard:judge_tcp]

Decides whether a TCP request may proceed. Arguments: the endpoint `family` (`legacy-rpc` or `json2`), the `GateSettings`, and `client` (an address or None). Returns `(allowed, reason)` with reason `allowed`, `transport` (settings do not serve TCP), `legacy-closed` (legacy family while `legacy_access=none`) or `network` (client unknown or outside `allowed_networks`). Checks run in that order. Pure, no I/O. Tests: `test_allowed_and_denied_networks`, `test_legacy_switch_and_transport`.

### `judge_unix(family, settings, peer) -> (bool, str, frozenset)` [@ANCHOR: hams_rpc_guard:judge_unix]

Decides whether a request on the Unix socket may proceed. Arguments: `family`, `settings`, and `peer` (a `Peer`). Returns `(allowed, reason, logins)`: reason `transport` (settings do not serve Unix), `legacy-closed`, `peer` (no `unix_peers` readable, or no rule matches) or `allowed`; `logins` is the frozenset of Odoo logins the matching rule permits (empty means any), re-checked after authentication by `patch.check_rpc_call`. Pure, no I/O. Tests: `tests/test_unix_listener.py` `test_judge_unix`.

### `note_request(family, client_class, allowed, reason, served=None) -> int` [@ANCHOR: hams_rpc_guard:note_request]

Counts one judged request and logs refusals. Arguments: `family`, `client_class` (from `network_class`, or `unix`), `allowed`, `reason`, and `served` (default `allowed`; in report mode a would-be refusal is served and counted as `would-refuse:<reason>`). Increments a per-worker counter under a lock and returns it; a refusal is logged at WARNING on the first and every power of ten after it, naming only family, class, reason and count, never an address, login, header or body. Tests: `test_counters_are_by_class_and_the_log_carries_no_value`.

### `request_counts() -> dict` [@ANCHOR: hams_rpc_guard:request_counts]

Returns a copy of this worker's counters, `{(family, network class, "allowed" | "refused:<reason>" | "would-refuse:<reason>"): count}`, taken under the same lock `note_request` uses so the copy is consistent. No arguments, no side effects. Counters are per process and reset when the worker restarts; they are not shared between workers. Used by the tests. Tests: `test_counters_are_by_class_and_the_log_carries_no_value`.

### `is_power_of_ten(count) -> bool` [@ANCHOR: hams_rpc_guard:is_power_of_ten]

True when `count` is 1, 10, 100, 1000 and so on. It decides which refusals reach the log: the first and every tenfold repeat, so a scanner hitting the gate a million times writes about seven lines. Argument: a positive int. Pure, no I/O, loops by dividing by ten. Zero or a negative number returns False. Tests: `test_counters_are_by_class_and_the_log_carries_no_value`.

## `hams_rpc_guard/models/ir_http.py`

### `IrHttp._authenticate(cls, endpoint)` [@ANCHOR: hams_rpc_guard:endpoint_gate]

The Odoo hook that runs the gate. Overrides `ir.http._authenticate` and calls `_refuse_rpc_endpoint_when_closed` before the core's own authentication, so a refused `/json/2` request never has its bearer key checked and a refused caller learns nothing about whether a key was valid. Argument `endpoint`: the matched route, passed through to `super`. Returns whatever the core returns; a refusal raises `werkzeug.exceptions.NotFound` (404). Runs per request, on the request's worker thread, no lock of its own. Tests: `test_default_a_daemon_on_this_machine_still_works`, `test_default_a_forwarded_request_is_refused_on_every_endpoint`.

### `IrHttp._refuse_rpc_endpoint_when_closed(cls)` [@ANCHOR: hams_rpc_guard:refuse_endpoint_when_closed]

The gate itself. Takes no arguments; reads the current request. Does nothing for a path that is not an endpoint family. Reads `hams_rpc_guard.mode` and the settings through `zero_sudo.security.utils`; if that raises, it logs the exception and uses the strictest settings (no network, legacy `none`, transport `closed`). Mode `off` returns. A request carrying a Unix peer in the environ is judged by `judge_unix`, otherwise `resolve_client` then `judge_tcp`; the result is counted by `note_request`, and unless allowed (or in report mode) it raises `NotFound`. Tests: `test_report_mode_serves_and_counts_and_off_is_stock`, `test_an_unreadable_parameter_fails_closed`.

## `hams_rpc_guard/models/ir_config_parameter.py`

### `PARSERS` map [@ANCHOR: hams_rpc_guard:parameter_parsers]

A module-level dictionary from each gate parameter name to the `endpoint_gate` parser that validates it: allowed networks (with the loopback default), trusted proxies, client-address header, legacy access, transport and unix peers. `_check_hams_rpc_guard_parameters` looks the written key up here; a key not in the map is not checked. Each parser raises `ConfigError` on a bad value, so a typo is refused at write time rather than silently leaving the endpoints closed or open. No I/O at import. Tests: `test_a_bad_value_cannot_be_written`.

### `IrConfigParameter._check_hams_rpc_guard_parameters(self)` [@ANCHOR: hams_rpc_guard:check_gate_parameters]

An `api.constrains("key", "value")` check on `ir.config_parameter`. For each record whose key is in `PARSERS`, it runs the parser on the stored value and converts a `ConfigError` into an Odoo `ValidationError` ("Invalid RPC guard setting: ..."), which aborts the create or write. Other keys are ignored. Runs inside the write's transaction, with no locking of its own. Security: refusing at write time means the fail-closed reading in `read_settings` is only a second line of defence. Tests: `test_a_bad_value_cannot_be_written`.

### `IrConfigParameter._flush_hams_rpc_guard_cache(self, keys)` [@ANCHOR: hams_rpc_guard:parameter_cache_flush]

Clears zero_sudo's cache after a gate setting changes. Argument `keys`: the parameter keys that were created, written or deleted. If any starts with `hams_rpc_guard.` it calls `_invalidate_model_cache("zero_sudo.security.utils")`, which clears the process cache and tells the other workers through Redis. Without it a changed mode, transport, peer list or network list would not reach a running worker for up to the cache lifetime (24 hours in Redis). The `create`, `write` and `unlink` overrides call it after `super`. Tests: `test_a_changed_setting_is_read_fresh_not_from_the_zero_sudo_cache`.

### `IrConfigParameter.create(self, vals_list)` [@ANCHOR: hams_rpc_guard:parameter_create_flush]

The `ir.config_parameter` `create` override. It creates the records through `super`, then calls `_flush_hams_rpc_guard_cache` with the new keys. Arguments: `vals_list`, the list of value dicts for the new parameters. Returns what `super().create` returns. Errors: whatever `super` raises (including the key/value validation `ValidationError`); the cache is not flushed if `super` raised. Side effects: when a key starts with `hams_rpc_guard.`, zero_sudo's cache of `_get_system_param` is cleared and the other workers are told through Redis. Runs in the caller's transaction, takes no lock of its own. Tests: `test_a_changed_setting_is_read_fresh_not_from_the_zero_sudo_cache`.

### `IrConfigParameter.write(self, vals)` [@ANCHOR: hams_rpc_guard:parameter_write_flush]

The `ir.config_parameter` `write` override. It collects the keys before the write (plus a new `key` value if one is being set), writes through `super`, then flushes the cache for all of them. Arguments: `vals`, the values to write. Returns what `super().write` returns. Errors: whatever `super` raises (including the key/value validation `ValidationError`); the cache is not flushed if `super` raised. Side effects: when a key starts with `hams_rpc_guard.`, zero_sudo's cache of `_get_system_param` is cleared and the other workers are told through Redis. Runs in the caller's transaction, takes no lock of its own. Tests: `test_a_changed_setting_is_read_fresh_not_from_the_zero_sudo_cache`.

### `IrConfigParameter.unlink(self)` [@ANCHOR: hams_rpc_guard:parameter_unlink_flush]

The `ir.config_parameter` `unlink` override. It collects the keys, deletes through `super`, then flushes the cache for them. Argument: none besides the records themselves. Returns what `super().unlink` returns. Errors: whatever `super` raises (including the key/value validation `ValidationError`); the cache is not flushed if `super` raised. Side effects: when a key starts with `hams_rpc_guard.`, zero_sudo's cache of `_get_system_param` is cleared and the other workers are told through Redis. Runs in the caller's transaction, takes no lock of its own. Tests: `test_a_changed_setting_is_read_fresh_not_from_the_zero_sudo_cache`.

## `hams_rpc_guard/patch.py`

### `unix_identity() -> (str, frozenset | None)` [@ANCHOR: hams_rpc_guard:unix_identity]

Names the local caller of a request that came in on the Unix socket and was admitted by the gate. No arguments; reads the current request's environ. Returns `("unix:<user name>", logins)` (or `unix:uid:<n>` when the uid has no name) with `logins` the frozenset of Odoo logins the peer may act as (empty means any); returns `("", None)` for any other call. Security: both environ entries are set only by the listener and the gate, never from a header. Tests: `test_identity_names_the_local_user`, `test_identity_is_absent_off_the_socket`.

## `hams_rpc_guard/hooks.py`

### `post_init_hook(env)` [@ANCHOR: hams_rpc_guard:post_init_hook]

Runs once when the module is installed. Argument `env`: the Odoo environment. Calls `ir.module.module._bootstrap_knowledge_docs()` to install the module's end-user documentation article. Returns nothing. It deliberately sets no system parameter: the defaults in code are the safe ones, and an operator's explicit choice must never be overwritten by an install or upgrade. Side effects: creates or updates knowledge articles in the database. Tests: `test_the_hook_installs_the_documentation_and_sets_no_parameter`.

## `hams_rpc_guard/unix_listener.py`

### Option names and defaults [@ANCHOR: hams_rpc_guard:listener_options]

Module constants for the server-configuration options that create the Unix socket: `hams_rpc_guard_socket` (absolute path), `hams_rpc_guard_socket_mode` (default `0660`), `hams_rpc_guard_socket_group` and `hams_rpc_guard_socket_owner`; the environment variable `HAMS_RPC_GUARD_SOCKET_FD` that carries the descriptor across a master reload; backlog 64; a 2-second socket timeout like Odoo's own worker. They are server-wide (file) settings because the socket exists before any database is open. Constants only. Tests: `tests/test_unix_listener.py` `test_config`.

### `listener_config(config) -> dict | None` [@ANCHOR: hams_rpc_guard:listener_config]

Reads the listener options. Argument `config`: `odoo.tools.config` or any mapping with `get`. Returns None when no path is set, else `{path, mode, group, owner}`. Raises `ListenerError` for a relative path, a mode that is not octal, or a mode other than `0600` or `0660` (any permission for others is refused). Pure, no I/O. Callers: `ensure_listener`, which logs the error and runs without the listener. Security: the mode limit keeps the socket from ever being world-accessible. Tests: `test_config`.

### `open_listener(path, mode=0o660, group="", owner="", backlog=64) -> socket` [@ANCHOR: hams_rpc_guard:open_listener]

Creates, binds and listens on the Unix socket `path`, returning the non-blocking listening socket. A stale socket file (connect fails) is removed and replaced; a live one, or a path that is not a socket, raises `ListenerError`, as do an unknown user or group, a missing directory, and a failed `bind`, `chown` or `chmod`. Binding happens under umask `0177`, so the file is never wider than 0600 before the final `chmod(mode)`. Side effects: creates a filesystem entry, may unlink a stale one. Needs root to give another owner. Tests: `test_socket_permissions_and_peer_credentials`, `test_a_live_socket_or_a_regular_file_is_never_replaced_but_a_stale_one_is`.

### `peer_of(connection) -> Peer` [@ANCHOR: hams_rpc_guard:peer_of]

Returns the verified `Peer` of an accepted `AF_UNIX` connection. Argument `connection`: the accepted socket. It reads `SO_PEERCRED` (pid, uid, gid, set by the kernel when the client connected) and looks up the user and group names in the passwd and group databases; a missing name becomes "". May raise `OSError` if the socket option cannot be read. Security: this is the identity the whole Unix transport rests on; nothing the client sends can alter it. Tests: `test_socket_permissions_and_peer_credentials`.

### `_user_names() -> dict` [@ANCHOR: hams_rpc_guard:user_names]

Returns `{uid: name}` from the passwd database, read whole with `pwd.getpwall()` (a uid without an entry is an ordinary answer, never an error). No arguments. It reads the database on every call, so a user added while the server runs is recognised, at the price of one scan per connection and per `open_listener`. Used by `peer_of` and to resolve the configured socket owner. No lock; read-only. Tests: `test_socket_permissions_and_peer_credentials`.

### `_group_names() -> dict` [@ANCHOR: hams_rpc_guard:group_names]

Returns `{gid: name}` from the group database with `grp.getgrall()`. No arguments, read-only, a fresh scan on every call. Used by `peer_of` to name the peer's primary group (for `@group` rules in `unix_peers`) and by `open_listener` to resolve the configured socket group. A gid without an entry simply has no name. Tests: `test_socket_permissions_and_peer_credentials`.

### `rpc_only_app(app) -> wsgi application` [@ANCHOR: hams_rpc_guard:rpc_only_app]

Wraps a WSGI application (normally `odoo.http.root`) for the Unix socket. Argument `app`. The returned application answers a plain 404 with a short text for any path that is not an RPC endpoint family, so the socket cannot reach the web client; for an endpoint path it removes every forwarding header from the environ (so Odoo's logs and `proxy_mode` never see an address the local caller invented) and calls `app`. No lock; runs per request. Tests: `test_only_the_rpc_endpoints_are_served_on_the_socket`, `test_forwarding_headers_cannot_change_the_identity`.

### `class UnixRequestHandler(WSGIRequestHandler)` [@ANCHOR: hams_rpc_guard:unix_request_handler]

werkzeug's request handler for one Unix-socket connection. One request per connection (HTTP/1.0), a per-socket-operation timeout of 2 seconds so an idle client cannot hold a worker, `REMOTE_ADDR` set to the loopback address (the caller is on this machine, so Odoo code parsing `remote_addr` keeps working) and the kernel-verified peer put in the environ. The gate keys on the environ entry, never on `REMOTE_ADDR`. Its three methods are documented separately. Tests: `tests/test_unix_listener.py` server tests.

### `UnixRequestHandler.address_string(self) -> str` [@ANCHOR: hams_rpc_guard:unix_request_handler_address_string]

Returns the loopback address text as the "client address" for werkzeug's access log, instead of the empty string an `AF_UNIX` peer would otherwise yield (which would break formatting). No arguments, no I/O, cannot fail. The value is a constant derived from the integer `0x7F000001`, written that way because the test linter refuses a literal loopback address in source. It is never used for any access decision.

### `UnixRequestHandler.port_integer(self) -> int` [@ANCHOR: hams_rpc_guard:unix_request_handler_port_integer]

Returns 0 as the "client port" for werkzeug, which expects the client address of a TCP connection to be a pair. An `AF_UNIX` peer has no port. No arguments, no I/O, cannot fail. Like `address_string`, it exists only to keep the server's logging and `REMOTE_PORT` handling from breaking, and plays no part in any access decision.

### `UnixRequestHandler.make_environ(self) -> dict` [@ANCHOR: hams_rpc_guard:unix_request_handler_make_environ]

Builds the WSGI environment of one request: werkzeug's standard `make_environ` plus `hams_rpc_guard.unix_peer`, the dictionary of `peer_of(self.connection)`. May raise `OSError` if the kernel credentials cannot be read, which fails that one request. Security: the key is not an `HTTP_` header, so no client can set it, and the gate's Unix branch reads only this entry. Tests: `test_only_listed_local_users_get_in`, `test_forwarding_headers_cannot_change_the_identity`.

### `make_wsgi_server(sock, app, threaded=False)` [@ANCHOR: hams_rpc_guard:unix_wsgi_server]

Builds a werkzeug server object on the listening socket. Arguments: `sock` (from `open_listener`), `app` (the Odoo WSGI application), `threaded` (True gives a `ThreadedWSGIServer` for `serve_forever` in a daemon thread, used when Odoo runs without workers; False gives a `BaseWSGIServer` whose `process_request` a prefork HTTP worker calls once per connection). The server serves `rpc_only_app(app)` through `UnixRequestHandler`, adopting the descriptor with `fd=`. Tests: `tests/test_unix_listener.py` server tests.

### `serve_unix_connection(server, connection)` [@ANCHOR: hams_rpc_guard:serve_unix_connection]

Serves one accepted connection with a server from `make_wsgi_server`. Arguments: the `server` and the accepted `connection`. It makes the connection blocking (with the handler's timeout), sets close-on-exec so a child process never inherits it, then calls `server.process_request`. A broken pipe (`EPIPE`) is swallowed, as Odoo's own worker does; any other `OSError` propagates. Runs in the prefork worker's accept loop, so the worker's time and memory limits cover the request. Tests: `test_served_on_the_socket_only_when_the_operator_says_so`.

### `listener_socket() -> socket | None` [@ANCHOR: hams_rpc_guard:listener_socket]

Returns this process's listening Unix socket, or None when none is open. No arguments. The state is per process (a forked worker has its own copy), read without the lock because a reference read is atomic. The patched worker methods use it to decide whether to add the socket to their `select` and to accept on it; a cron worker closes it and sets it back to None. Tests: `test_server_hooks_are_installed_once`.

### `ensure_listener(config) -> socket | None` [@ANCHOR: hams_rpc_guard:ensure_listener]

Opens the configured listener once per process and returns it. Argument `config`. Under a lock it returns an already open socket; otherwise reads `listener_config`, and either adopts the descriptor named by `HAMS_RPC_GUARD_SOCKET_FD` (a reloading master) or calls `open_listener`. No path configured returns None silently. Any `ListenerError`, `OSError` or `ValueError` is logged at ERROR and returns None: the TCP server still runs, and with `rpc_transport=unix` the endpoints then answer nobody (closed, never open). Tests: `test_ensure_listener_without_a_path_or_with_a_bad_one_opens_nothing`.

### `install_server_patches()` [@ANCHOR: hams_rpc_guard:install_server_patches]

Teaches Odoo's server classes about the listener; idempotent, called from `post_load` in the prefork master before any worker exists. Replaces `PreforkServer.start`, `process_spawn` and `fork_and_reload` (open the socket; pass the descriptor to a reloaded master), `WorkerHTTP.start`, `sleep` and `process_work` (select on both sockets, accept one Unix connection per loop, otherwise Odoo's own TCP accept), `WorkerCron.start` (close the descriptor) and `ThreadedServer.http_spawn` (a daemon thread). With no socket path configured it opens nothing. It logs an error if the module was first loaded in a worker. Tests: `test_server_hooks_are_installed_once`, `test_the_tcp_fallback_of_a_worker_runs_outside_the_except_block`.

## `zero_sudo/daemon/odoo_rpc_client.py`

### `class OdooRpcClient` [@ANCHOR: zero_sudo:odoo_rpc_client]

A small JSON-2 client for daemons, standard library only so any daemon can copy it. It talks to one Odoo database over TCP (`http://`, `https://`) or over the Unix socket (`unix:///absolute/path`), one new connection per call, sending the API key as a bearer token and the database name in `X-Odoo-Database`. On the socket the kernel also tells Odoo which local user is calling. Construct it with `url`, `api_key`, `database`, `timeout` and `host`; call it with `call`. The key is never logged. Tests: `tests/test_unix_listener.py` `test_the_client_helper_refuses_bad_urls`.

### `OdooRpcClient.__init__(self, url, api_key, database, timeout=30, host="unix-socket")` [@ANCHOR: zero_sudo:odoo_rpc_client_init]

Validates and stores the connection settings. `url`: `unix:///abs/path` (no host part) or `http(s)://host[:port][/prefix]`; `api_key`: the service account's key; `database`: header value; `timeout`: seconds per socket operation; `host`: the Host header on the Unix socket (set it when the server's `dbfilter` uses `%h` or `%d`). Raises `ValueError` for a relative or host-bearing Unix path, an unsupported scheme, or a missing hostname. No I/O. Tests: `test_the_client_helper_refuses_bad_urls`.

### `OdooRpcClient._connection(self)` [@ANCHOR: zero_sudo:odoo_rpc_client_connection]

Returns a new, unconnected `http.client` connection for the configured URL: a `UnixHTTPConnection` for `unix://`, an `HTTPSConnection` for `https` and an `HTTPConnection` otherwise, each with the client's timeout. No arguments, no network I/O until the request is made. A new object is made for every `call`, so the client holds no connection state and is safe to share between threads. Tests: `tests/test_unix_listener.py` client tests.

### `OdooRpcClient.call(self, model, method, **params)` [@ANCHOR: zero_sudo:odoo_rpc_client_call]

POSTs `params` as a JSON body to `/json/2/<model>/<method>` (URL-quoted; any URL path prefix kept for TCP) with the bearer key and database headers, and returns the decoded JSON answer (None for an empty body). Raises `OdooRpcError` for any status other than 200: 404 when the guard refused the caller or the route is missing, 401 for a bad key, 403 or 422 for an access or argument error. Raises `OSError` when the socket cannot be reached. The connection is always closed. Tests: `test_served_on_the_socket_only_when_the_operator_says_so`.

### `class UnixHTTPConnection(HTTPConnection)` [@ANCHOR: zero_sudo:unix_http_connection]

An `http.client.HTTPConnection` that connects to a Unix socket path instead of a TCP address. The `Host` header is `unix-socket` unless a host is given, because Odoo serves no virtual host by it on the socket. Everything else (request, response, close) is the standard library's. Constructor and `connect` are documented separately. Used by `OdooRpcClient` for `unix://` URLs; no lock, one connection per object.

### `UnixHTTPConnection.__init__(self, path, timeout=30, host="unix-socket")` [@ANCHOR: zero_sudo:unix_http_connection_init]

Constructor. Arguments: `path` (absolute socket path, stored as `socket_path`), `timeout` (seconds, applied to connect and every read), `host` (value for the `Host` header, passed to the base class as the host name). It makes no connection and no system call; the base class stores the host and timeout. Cannot fail on a string path. Tests: the client tests in `tests/test_unix_listener.py`.

### `UnixHTTPConnection.connect(self)` [@ANCHOR: zero_sudo:unix_http_connection_connect]

Opens the `AF_UNIX` stream socket to `socket_path` with the configured timeout and stores it in `self.sock`, which `http.client` then uses. On `OSError` (no such path, permission refused, connection refused, timeout) it closes the half-open socket and re-raises, so no descriptor leaks. The kernel records the calling process's uid, gid and pid at this `connect()`, which is what the server later reads as the caller's identity. Tests: the client tests in `tests/test_unix_listener.py`.

### `OdooRpcError.__init__(self, status, body)` [@ANCHOR: zero_sudo:odoo_rpc_error_init]

Constructor of the exception `call` raises. Arguments: `status` (the int HTTP status, 0 when no answer arrived) and `body` (the first 500 characters of the answer, for the daemon's own log; Odoo's error answers carry no secret). Sets the message "JSON-2 call failed with HTTP <status>: <body>" and keeps both values as attributes so a caller can branch on `status`. No I/O. Tests: `test_served_on_the_socket_only_when_the_operator_says_so`.

## Verification

`tests/test_endpoint_gate.py` makes real HTTP requests with and without forwarding headers (forged ones from a peer
that is not a trusted proxy included) under the settings and checks the answers (200, 401 or 404), the fail-closed
reading and the write-time validation, and that the log carries no request value. `tests/test_unix_listener.py` runs
the listener on a scratch socket in the test process and calls it with the daemon client. `tools/prefork_unix_check.py`
runs a throwaway Odoo with `--workers 2` (scratch database cluster, port and socket) and checks the prefork path.
