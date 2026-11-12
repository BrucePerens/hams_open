# HAMS RPC Guard (`hams_rpc_guard`)

*Copyright © Bruce Perens K6BP.*
SPDX-License-Identifier: AGPL-3.0-or-later

Odoo's generic RPC endpoints let anyone who holds a login, a password or an API key call any public method of any
model. This module makes them answer only callers on networks the operator allows (by default: the machine Odoo runs
on), optionally only on a Unix socket whose callers the kernel identifies, and limits which methods each caller may use.
It works on a stock Odoo 19 Community server (it needs only core Odoo, `rpc`, `zero_sudo` and `knowledge`), and its
defaults are the safe ones with no configuration.

## Contents

1. [What it does](#what-it-does)
2. [Settings](#settings)
3. [Who the true client is](#who-the-true-client-is)
4. [Operators: a reverse proxy for another network](#operators-a-reverse-proxy-for-another-network)
5. [Operators: nginx or Cloudflare instead, or as well](#operators-nginx-or-cloudflare-instead-or-as-well)
6. [Unix socket](#unix-socket)
7. [Transport and identity: recommendation](#transport-and-identity)
8. [Anchor entries](#anchor-entries)

## What it does

| Endpoint | Family | Default |
|---|---|---|
| `/xmlrpc/<service>`, `/xmlrpc/2/<service>`, `/jsonrpc` | `legacy-rpc` | callers on `allowed_networks` (loopback); `legacy_access = none` closes them |
| `/json/2/<model>/<method>` | `json2` | callers on `allowed_networks` (loopback) |

A refused request receives a plain 404, as if the route did not exist. The refusal runs before Odoo checks the
credentials, so a refused JSON-2 request never has its bearer key looked at. `/web/dataset/call_kw` and
`/web/dataset/call_button` (the browser's own calls) are **not** external RPC endpoints: the endpoint gate does not
touch them, and they stay on TCP in every transport setting. Browsers cannot be limited by network; their calls are
limited by the method whitelist below.

`hams_rpc_guard.mode` (see the whitelist's parameters) governs both halves: `enforce` (default) refuses, `report`
serves every request and call but counts and logs what would have been refused, `off` is stock Odoo.

The public (anonymous) visitor cannot use `call_kw` or `call_button` at all: Odoo registers both routes `auth="user"`
and answers the public user with "session expired". `tests/test_endpoint_gate.py` pins that, so a future route or
`auth` override that lets the public user through fails a test.

## Settings

System parameters (Settings, Technical, System Parameters). None is created by the module; unset means the default.
A value that does not parse is refused when it is saved (a `ValidationError` naming the setting); one that reaches the
database anyway (SQL, an import) is logged at ERROR on every request and read as its closed value, never as open.

| Parameter | Values | Default | Closed value | Meaning |
|---|---|---|---|---|
| `hams_rpc_guard.allowed_networks` | comma-separated IPv4/IPv6 networks (CIDR) or host addresses, or `none` | `127.0.0.0/8,::1/128` | `none` | Callers on these networks may use the endpoints over TCP. |
| `hams_rpc_guard.legacy_access` | `networks`, `none` | `networks` | `none` | `none` closes `/xmlrpc...` and `/jsonrpc` everywhere (TCP and socket). |
| `hams_rpc_guard.trusted_proxies` | comma-separated networks | empty | empty | Reverse proxies whose client-address header is believed. |
| `hams_rpc_guard.client_address_header` | `CF-Connecting-IP`, `X-Real-IP` or `True-Client-IP` | empty | empty | The header a trusted proxy *sets* (overwriting the client's) to the true client address. `X-Forwarded-For` is refused: the client writes its first entries. |
| `hams_rpc_guard.rpc_transport` | `tcp`, `unix`, `both` | `tcp` | no transport | Where the endpoints are served. `unix` refuses them on TCP entirely. |
| `hams_rpc_guard.unix_peers` | comma-separated `WHO[=LOGIN\|LOGIN...]` | empty (no one) | no one | Local users admitted on the Unix socket, and the Odoo logins each may act as. |

Server configuration file options (`odoo.conf`, `[options]`; read once at start; Odoo logs each as "unknown option ...
stored as-is", which is expected):

| Option | Default | Meaning |
|---|---|---|
| `hams_rpc_guard_socket` | unset (no socket) | Absolute path of the Unix socket, for example `/run/odoo/rpc.sock`. |
| `hams_rpc_guard_socket_mode` | `0660` | `0600` (only the Odoo user) or `0660` (also the group). Anything else is a start-up error. |
| `hams_rpc_guard_socket_group` | the Odoo user's group | The group that may connect (the Odoo user must be a member). |
| `hams_rpc_guard_socket_owner` | the Odoo user | Another owner needs Odoo to start as root; normally leave unset. |

Requests are counted per worker by endpoint family, network class (`loopback`, `private`, `link-local`, `public`,
`unknown`, `unix`) and outcome; refusals are logged at the first and at every power of ten: never an address, login,
model, method, header or body.

## Who the true client is

Behind nginx or Cloudflare Tunnel the TCP peer of every request is a loopback address, so the peer alone proves
nothing. `resolve_client` decides:

1. The socket peer is read from werkzeug's ProxyFix record `werkzeug.proxy_fix.orig` when `proxy_mode` has rewritten
   `REMOTE_ADDR` (the rewritten value came from a header the client controls).
2. A request with none of `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto`, `X-Forwarded-Server`,
   `X-Real-IP`, `Forwarded`, `Via`, `CF-Connecting-IP`, `CF-Ray`, `True-Client-IP` is direct: the peer is the client.
3. A request with any of them, from a peer inside `trusted_proxies`, has as its client the single address in
   `client_address_header`; missing, sent twice, a list, or not an address: unknown.
4. Any other request with a forwarding header: unknown. Unknown is on no network, so it is refused.

A visitor can add `X-Real-IP: 127.0.0.1` but cannot become a trusted proxy. The proxy that is trusted must set the
header on every request it forwards (nginx `proxy_set_header` replaces what the client sent), and no untrusted path may
reach Odoo from the trusted proxy's address. Limitation: a proxy that adds no forwarding header cannot be told from a
direct caller, which is why Odoo's deployment guide requires `proxy_mode` and the `X-Forwarded-*` headers.

## Operators: a reverse proxy for another network

Example: daemons on a WireGuard network `10.99.0.0/24` reach Odoo (bound to `127.0.0.1:8069`) through nginx on the
WireGuard address, while the public site arrives through another nginx or Cloudflare Tunnel on the same host. The
WireGuard nginx connects to Odoo from its own loopback source address, so it is the only trusted proxy:

```nginx
server {
    listen 10.99.0.1:8070;                      # the WireGuard address only
    location ~ ^/(json/2/|jsonrpc$|xmlrpc/) {
        proxy_bind 127.0.0.2;                   # the trusted proxy's own source address
        proxy_set_header X-Real-IP $remote_addr;              # overwrites whatever the client sent
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Host $host;
        proxy_pass http://127.0.0.1:8069;
    }
    location / { return 404; }
}
```

and `allowed_networks = 127.0.0.0/8,::1/128,10.99.0.0/24`, `trusted_proxies = 127.0.0.2/32`,
`client_address_header = X-Real-IP`. A visitor through the public proxy (peer `127.0.0.1`, not trusted, with forwarding
headers) is unknown and refused, whatever headers it forges.

## Operators: nginx or Cloudflare instead, or as well

The module is the default so that every installer is protected. An operator can add the same refusal at the edge:

```nginx
location ~ ^/(xmlrpc/|jsonrpc$) { return 404; }
```

On Cloudflare, a WAF custom rule `(starts_with(http.request.uri.path, "/xmlrpc/")) or (http.request.uri.path eq "/jsonrpc")`
with action Block (the `cloudflare` module's default WAF rules already block `/xmlrpc`). Both are stopgaps in the
sense of the project's security rule: they protect only the hosts that have them; the module is what protects everyone.

## Unix socket

Odoo 19's own HTTP server creates only TCP sockets, but it is a WSGI application behind a small accept loop, so the
module adds a second listener of its own (`unix_listener.py`) that hands each connection to the same application and
puts the kernel-verified credentials of the connecting process (`SO_PEERCRED`: pid, uid, gid) in the request. Only the
external RPC endpoints are served on it (every other path answers 404 there), forwarding headers are dropped, and
`REMOTE_ADDR` is the loopback address so Odoo code that parses it keeps working; the gate keys on the credentials.

How it fits each Odoo run mode (verified on Odoo 19, see `tools/prefork_unix_check.py`):

| Run mode | Where the listener lives | Limits |
|---|---|---|
| prefork (`--workers N`) | Created in the master before the first worker is forked (`PreforkServer.start`, or `process_spawn` when the module was loaded while preloading `db_name`); every HTTP worker inherits the descriptor and waits on it beside Odoo's TCP socket in its own `select`, one request per loop. | The worker's watchdog (`limit_time_real`), CPU limit (`limit_time_cpu`), memory limits and `limit_request` apply exactly as to TCP requests; a 2-second socket timeout as Odoo's own. Cron workers close the descriptor. A `SIGHUP` reload hands it to the new master (`HAMS_RPC_GUARD_SOCKET_FD`). |
| threaded (no `--workers`) | A werkzeug threaded server in a daemon thread beside Odoo's `odoo.service.httpd`. | As Odoo's threaded server. |
| gevent (websocket process) | None: that process serves websockets only, and it does not inherit the descriptor. | |
| `odoo shell`, `--stop-after-init` | None. | |

The module must be loaded in the prefork master: add it to `server_wide_modules` (for example
`server_wide_modules = base,rpc,web,hams_rpc_guard`), or set `db_name` so the registry is preloaded there. If it is
first loaded in a worker, an ERROR says so and no socket is created (with `rpc_transport = unix` the endpoints then
answer no one: closed, not open). A live socket at the path (another server) or a path that is not a socket stops the
listener with an ERROR; a stale socket file is replaced; the file is created under umask `0177`.

**Permissions.** Two gates: the filesystem (owner, group, mode: only members of the socket's group can connect at all)
and `hams_rpc_guard.unix_peers`. `WHO` is a user name, a numeric uid, `@group` or `@gid` (the peer's *primary* group:
`SO_PEERCRED` carries no supplementary groups); `=LOGIN|LOGIN` binds it to those Odoo logins (a call authenticated as
any other account is refused with an access error). Example for systemd:

```ini
# odoo.service
[Service]
RuntimeDirectory=odoo
RuntimeDirectoryMode=0750
SupplementaryGroups=hamsd
```

```ini
# odoo.conf
hams_rpc_guard_socket = /run/odoo/rpc.sock
hams_rpc_guard_socket_group = hamsd
hams_rpc_guard_socket_mode = 0660
```

and `hams_rpc_guard.unix_peers = hamsd_backup=backup_service, hamsd_pager=pager_service`,
`hams_rpc_guard.rpc_transport = unix`. The directory must let the daemon users traverse it (group `hamsd`, mode 0750).
The identity `unix:<user>` can also be given a `clients` entry in `rpc_exposure.json`, which narrows the daemon like a
certificate subject (both lists must allow a pair).

**Client.** `zero_sudo/daemon/odoo_rpc_client.py` (`OdooRpcClient("unix:///run/odoo/rpc.sock", api_key, db)`,
standard library only) speaks JSON-2 over the socket or over TCP. The API key is still required: the socket proves which
local user is calling, the key which Odoo account.

## Transport and identity

**On the Odoo host**: the Unix socket with `rpc_transport = unix` and a `unix_peers` entry per daemon user bound to its
service login. A process of another local user cannot connect (filesystem) or is refused (`unix_peers`), even with a
stolen key; nothing answers the endpoints on TCP.

**Off the host**: a private network (WireGuard) and the reverse-proxy pattern above, or mutual TLS at nginx
(`hams_rpc_guard.client_cert_header`, below). Never a public network on `allowed_networks`.

**TLS on loopback** adds little against a local attacker: a process that can read a daemon's memory already has its key.
The socket's kernel identity is the stronger on-box control. This module creates and stores no key or certificate;
nothing here touches a backup key. Do not run daemons as root or as the `odoo` user (the Odoo user can always connect).

## Anchor entries

### `hams_rpc_guard:gate_parameters`

**Purpose.** The names and words of the endpoint gate, in `endpoint_gate.py`: the six parameter names, the default
network list `DEFAULT_ALLOWED_NETWORKS`, the words `none`/`networks`, the transports `tcp`/`unix`/`both` and the
internal `closed`, `CLIENT_ADDRESS_HEADERS`, the families `legacy-rpc`/`json2`, the network classes, the environ keys
`UNIX_PEER_ENVIRON_KEY` (set by the listener to the peer's credentials dict) and `UNIX_LOGINS_ENVIRON_KEY` (set by the
gate to the bound logins), and `FORWARDING_HEADERS`. Constants; read by the gate, the listener, the parameter check
and the tests. The environ keys have no `HTTP_` prefix, so no request header can produce them.

### `hams_rpc_guard:endpoint_family`

**Purpose.** Classifies a request path as one of the two guarded endpoint families, or neither. Called for every
request by `ir.http._authenticate` and by the Unix-socket application.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | The request path without the query string. Anything that is not a `str` is "neither". |

**Returns.** `"legacy-rpc"` for exactly `/jsonrpc` and anything under `/xmlrpc/`; `"json2"` for `/json/2` and anything
under `/json/2/`; `None` otherwise (`/xmlrpc.php`, `/jsonrpc/`, `/json/21`, `/json/version`, `/web/dataset/...`).
**Errors.** None. **Concurrency.** Pure. **Test:** `test_endpoint_gate.TestEndpointFamily`.

### `hams_rpc_guard:parse_address`

**Purpose.** One header token or peer string to an address. `_parse_address(text)`: `text` any value (plain address,
`host:port`, `[v6]:port`, with a `%zone`); returns an `IPv4Address`/`IPv6Address` (an IPv4-mapped IPv6 address becomes
IPv4) or `None`. Pure. **Test:** `test_endpoint_gate.TestResolveClient`.

### `hams_rpc_guard:parse_networks`, `parse_trusted_proxies`, `parse_client_address_header`, `parse_legacy_access`, `parse_transport`

**Purpose.** Parse one setting each; used when a value is written (`check_gate_parameters`) and when it is read
(`read_settings`).

| Function | Argument | Returns | Raises `ConfigError` when |
|---|---|---|---|
| `parse_networks(value, param=ALLOWED_NETWORKS_PARAM, default="")` | the text; `param` names the setting in the message; `default` replaces unset/blank | tuple of `ip_network` (`none`: empty) | any entry is not an address or CIDR network |
| `parse_trusted_proxies(value)` | the text | tuple of `ip_network` (unset: empty) | as above |
| `parse_client_address_header(value)` | the text | canonical header name or `""` | not one of `CLIENT_ADDRESS_HEADERS` |
| `parse_legacy_access(value)` | the text | `networks` or `none` | anything else |
| `parse_transport(value)` | the text | `tcp`, `unix` or `both` | anything else |

All are pure and case-insensitive for the words. `ConfigError` is a `ValueError`. **Test:**
`test_endpoint_gate.TestSettings`.

### `hams_rpc_guard:peer`, `peer_rule`, `peer_rules`, `parse_peer_rules`

**Purpose.** The Unix-socket identity and its admission rules (`endpoint_gate.py`).

| Structure | Fields |
|---|---|
| `Peer` | `pid`, `uid`, `gid` (int, from `SO_PEERCRED`), `user`, `group` (names, `""` when none). `as_dict()` gives the environ form. Created per connection by `unix_listener.peer_of`. |
| `PeerRule` | `kind` (`user`, `uid`, `group`, `gid`), `value` (name or int), `logins` (frozenset; empty = any). `matches(peer)` compares one field. |
| `PeerRules` | `rules` (tuple of `PeerRule`, in the written order); `match(peer)` returns the first matching rule or `None`. |

`parse_peer_rules(value)` reads `hams_rpc_guard.unix_peers`: comma-separated `WHO[=LOGIN|LOGIN...]`, `WHO` a user
name, uid, `@group` or `@gid`. Unset or blank: no rule (everyone refused). Raises `ConfigError` on an empty `WHO`, a
name with characters other than letters, digits, `_ . -` (and a trailing `$`), or an empty/malformed login list.
Names are not looked up when parsing (the user may not exist yet on this host). Pure. **Test:**
`test_unix_listener.TestPeerRules`.

### `hams_rpc_guard:gate_settings` and `hams_rpc_guard:read_settings`

**Purpose.** `GateSettings` holds one database's parsed settings: `allowed_networks`, `trusted_proxies` (tuples of
networks), `client_address_header` (str), `legacy` (`networks`/`none`), `transport` (`tcp`/`unix`/`both`/`closed`),
`unix_peers` (`PeerRules`, or `None`: no peer admitted). Built per request; never shared or mutated after.
`read_settings(get_param)` builds it from `get_param(key, default)` (the zero_sudo parameter reader, cached). Every
value that does not parse is logged at ERROR and replaced by its closed value; a reader that raises propagates (the
gate then fails closed). **Test:** `test_endpoint_gate.TestSettings`.

### `hams_rpc_guard:socket_peer` and `hams_rpc_guard:resolve_client`

**Purpose.** The true client of a TCP request (rules in [Who the true client is](#who-the-true-client-is)).

| Argument | Type | Meaning |
|---|---|---|
| `environ` | `dict` | WSGI environ (`REMOTE_ADDR`, `werkzeug.proxy_fix.orig`). |
| `headers` | werkzeug `Headers`/`EnvironHeaders` | Case-insensitive; `getlist` is used to catch a header sent twice. |
| `trusted_proxies` | tuple of `ip_network` | From the setting. |
| `client_address_header` | `str` | Canonical name or `""`. |

**Returns.** `socket_peer(environ)`: the peer address or `None`. `resolve_client(...)`: the client address or `None`
(unknown). **Errors.** None. **Concurrency.** Pure. **Security.** A header value is believed only from a trusted
proxy, only from the one configured header, only as a single address. **Test:** `test_endpoint_gate.TestResolveClient`,
`TestRpcEndpointGate`.

### `hams_rpc_guard:network_class`

`network_class(address)` returns the counter class: `loopback`, `link-local`, `private` (RFC 1918, ULA, and the
documentation ranges Python counts as private), `public`, or `unknown` for `None`. Pure. **Test:**
`test_endpoint_gate.TestJudgement`.

### `hams_rpc_guard:judge_tcp` and `hams_rpc_guard:judge_unix`

**Purpose.** The decision. `judge_tcp(family, settings, client)` returns `(allowed, reason)`: refused with `transport`
when the transport is not `tcp`/`both`, `legacy-closed` for a legacy family under `legacy_access = none`, `network` when
the client is unknown or on no allowed network. `judge_unix(family, settings, peer)` returns `(allowed, reason,
logins)`: `transport` unless `unix`/`both`, `legacy-closed`, `peer` when no rule matches or the rules could not be read;
`logins` is the matching rule's frozenset. Pure. **Test:** `test_endpoint_gate.TestJudgement`,
`test_unix_listener.TestPeerRules`.

### `hams_rpc_guard:note_request`, `hams_rpc_guard:request_counts`, `hams_rpc_guard:is_power_of_ten`

**Purpose.** Counters. `note_request(family, client_class, allowed, reason, served=None)` counts under
`(family, class, "allowed" | "refused:<reason>" | "would-refuse:<reason>")` (`served` defaults to `allowed`; in report
mode a refusal is served) and logs a refusal at counts 1, 10, 100, ...; returns the count. `request_counts()` returns a
copy. Per worker process, under a lock; never persisted. **Test:** `test_endpoint_gate.TestJudgement`.

### `hams_rpc_guard:endpoint_gate` and `hams_rpc_guard:refuse_endpoint_when_closed`

**Purpose.** `ir.http._authenticate`, extended: before the core authenticates, a guarded family is judged. Reads
`hams_rpc_guard.mode` and the settings through `zero_sudo.security.utils._get_system_param`; `off` skips the gate; a Unix
request (environ entry set by the listener) is judged by `judge_unix` and, when admitted, gets its bound logins in the
environ; a TCP request by `resolve_client` and `judge_tcp`. A refusal raises werkzeug `NotFound` (404) in `enforce`
mode and is only counted in `report` mode. A parameter read failure is logged and read as closed. No database write.
**Test:** `test_endpoint_gate.TestRpcEndpointGate`, `TestPublicVisitorCannotUseTheWebClientRpc`,
`test_unix_listener.TestUnixSocketEndToEnd`.

### `hams_rpc_guard:check_gate_parameters` and `hams_rpc_guard:parameter_parsers`

**Purpose.** `ir.config_parameter`, extended with a constraint on `key`/`value`: a record whose key is in `PARSERS`
(the six gate settings, each mapped to its parser) must parse, or the create/write raises `ValidationError` naming the
setting. Other keys are untouched. **Test:** `test_endpoint_gate.TestSettings.test_a_bad_value_cannot_be_written`.

### `hams_rpc_guard:listener_options` and `hams_rpc_guard:listener_config`

**Purpose.** The server-file options (`OPTION_PATH`, `OPTION_MODE`, `OPTION_GROUP`, `OPTION_OWNER`; constants
`DEFAULT_MODE` 0660, `INHERITED_FD_ENV`, `BACKLOG` 64, `SOCKET_TIMEOUT` 2 s, `LOOPBACK_ADDRESS`).
`listener_config(config)`: `config` is `odoo.tools.config` or a mapping with `get`; returns `None` (no path) or
`{"path", "mode", "group", "owner"}`; raises `ListenerError` for a relative path or a mode other than 0600/0660. Pure.
**Test:** `test_unix_listener.TestListenerSocket`.

### `hams_rpc_guard:open_listener`

**Purpose.** Create the listening socket. `open_listener(path, mode=0o660, group="", owner="", backlog=64)` returns a
non-blocking `AF_UNIX` listening socket. **Errors.** `ListenerError` when the path exists and is not a socket, when
another server accepts on it, when the directory is missing or not writable, for an unknown owner/group, or when
chown/chmod fails (the file is then removed). A stale socket file is unlinked first. **Side effects.** Creates the file
under umask 0177, then chown and chmod. **Test:** `test_unix_listener.TestListenerSocket`.

### `hams_rpc_guard:peer_of`

`peer_of(connection)`: the `Peer` of an accepted `AF_UNIX` connection (`SO_PEERCRED`; names from the whole passwd and
group databases, `""` when absent). **Test:** `test_unix_listener.TestListenerSocket`.

### `hams_rpc_guard:rpc_only_app`, `unix_request_handler`, `unix_wsgi_server`, `serve_unix_connection`

**Purpose.** Serving a socket connection. `rpc_only_app(app)` wraps a WSGI app: 404 for any path that is not an
endpoint family, forwarding headers dropped. `UnixRequestHandler` (werkzeug's handler): HTTP/1.0, one request per
connection, 2 s per socket operation, `REMOTE_ADDR` loopback, the `Peer` dict in the environ.
`make_wsgi_server(sock, app, threaded=False)` returns a werkzeug `BaseWSGIServer` (prefork: `process_request` per
connection) or `ThreadedWSGIServer` (`serve_forever` in a thread) on a duplicate of `sock`.
`serve_unix_connection(server, connection)` serves one accepted connection, blocking, close-on-exec, `EPIPE` tolerated.
**Test:** `test_unix_listener.TestUnixSocketEndToEnd`.

### `hams_rpc_guard:listener_socket`, `hams_rpc_guard:ensure_listener`, `hams_rpc_guard:install_server_patches`

**Purpose.** Process wiring (table in [Unix socket](#unix-socket)). `listener_socket()` returns this process's socket
or `None`. `ensure_listener(config)` opens it once per process (or adopts the descriptor from `HAMS_RPC_GUARD_SOCKET_FD`
after a reload) and returns it, or `None` with an ERROR when it cannot; takes a lock. `install_server_patches()` wraps
`PreforkServer.start`, `process_spawn`, `fork_and_reload`, `WorkerHTTP.start`, `sleep`, `process_work`,
`WorkerCron.start` and `ThreadedServer.http_spawn` once (idempotent); called from `post_load_hook`. Opens nothing unless
`hams_rpc_guard_socket` is set. **Test:** `test_unix_listener.TestListenerSocket`, `tools/prefork_unix_check.py`.

### `hams_rpc_guard:unix_identity`

`unix_identity()` returns `("unix:<user>" or "unix:uid:<n>", bound logins)` for a call admitted on the socket, else
`("", None)`. `check_rpc_call` uses the identity like a certificate subject and refuses (reason
`unix-login-not-bound`) a call authenticated as a login the peer is not bound to. **Test:**
`test_unix_listener.TestUnixLoginBinding`.

### `hams_rpc_guard:post_init_hook`

**Purpose.** Installs the module's documentation article (`data/documentation.html`) through the knowledge module.
Takes the environment; returns nothing; sets no system parameter, so an operator's choice is never overwritten and
the safe defaults are the ones in code. **Test:** `test_endpoint_gate.TestPostInitHook`.

### `zero_sudo:odoo_rpc_client`, `zero_sudo:odoo_rpc_client_call`, `zero_sudo:unix_http_connection`

**Purpose.** The daemon client (`zero_sudo/daemon/odoo_rpc_client.py`). `OdooRpcClient(url, api_key, database,
timeout=30)`: `url` `unix:///abs/path`, `http://` or `https://` (else `ValueError`). `call(model, method, **params)`
POSTs JSON to `/json/2/<model>/<method>` with `Authorization: Bearer` and `X-Odoo-Database`, returns the decoded
answer, raises `OdooRpcError(status, body)` for any status but 200 (404: refused by the guard or no such route) and
`OSError` when unreachable. Blocking; one connection per call; never logs the key. `UnixHTTPConnection(path, timeout)`
is `http.client.HTTPConnection` over `AF_UNIX`. **Test:** `test_unix_listener.TestUnixSocketEndToEnd`.

## The method whitelist

Odoo core refuses an RPC call to a method whose name starts with `_` and to one marked `@api.private`. Everything else
public is callable by any signed-in user over `/web/dataset/call_kw`. This module adds a listing: a method of one of
*our* addons is callable over RPC only if it is declared. See
[`docs/stories/rpc_methods_need_a_listing.md`](docs/stories/rpc_methods_need_a_listing.md) for the rules and the anchors.

### Parameters

| Parameter | Values | Default | Meaning |
|---|---|---|---|
| `hams_rpc_guard.mode` | `enforce`, `report`, `off` | `enforce` | `enforce` refuses a call the rules deny. `report` serves it and counts it (the log shows `would deny`, per `model.method`, at the first and every power of ten). `off` judges nothing. An unreadable or unknown value is `enforce`. |
| `hams_rpc_guard.strict_stock` | `1`/`true`/`yes`/`on` | off | Internal users must also have stock methods declared (`stock` or `internal`). |
| `hams_rpc_guard.client_cert_header` | a header name | empty (off) | The header in which a trusted proxy puts the verified client-certificate subject; becomes the identity `cert:<subject>`. Believed only from a peer listed in `hams_rpc_guard.trusted_proxies`. |
| `hams_rpc_guard.managed_authors` | comma-separated | `Bruce Perens, Hams, HAMS` | A manifest whose author contains one of these, or an addon that ships `rpc_exposure.json`, is "ours". An installer with other addons adds the author name, or ships a file. |

**Rolling out on an existing site: set `hams_rpc_guard.mode` to `report` first**, run for a few days of real traffic,
read the log for `RPC guard (report)` lines, declare what is legitimate, then set `enforce`. A fresh install enforces.

### `rpc_exposure.json`

```json
{
  "internal": {"model.name": ["method"]},
  "portal":   {"model.name": ["method"]},
  "stock":    {"model.name": ["method"]},
  "clients":  {"service_login": {"sources": ["addon/daemon/main.py"], "allow": {"model.name": ["method"]}}}
}
```

| Section | Meaning |
|---|---|
| `internal` | Methods of this addon's models that signed-in internal users may call. Generated; shrinks as methods become private. |
| `portal` | Pairs a portal or public user may call. Everything else is refused to them, stock models included. This module ships the baseline the hams.com members' browsers use today (found in the access logs and the static JavaScript). |
| `stock` | With `strict_stock`: stock methods internal users may call. |
| `clients` | Per daemon: its service account's login, the daemon source files, and the pairs it may call. A listed daemon is refused every other pair. A service account with no entry is judged as an internal user and shows as `unmapped-client-...` in the report. |

`@public_rpc`, `@public_rpc(portal=True)` and `@public_rpc(clients=("login",))` declare the same thing in code.

### Per-daemon identity (mutual TLS for off-box daemons)

nginx in front of Odoo: `ssl_verify_client on; ssl_client_certificate <operator CA>;` and
`proxy_set_header X-Client-Cert-Subject $ssl_client_s_dn;` (nginx overwrites any value the client sent). Then set
`hams_rpc_guard.client_cert_header = X-Client-Cert-Subject` and `hams_rpc_guard.trusted_proxies = 127.0.0.1`, and give
the daemon an entry under `"cert:<subject DN>"` in a `clients` section. The subject can only narrow a daemon that also
has a login entry: both lists must allow the pair.

### Per-daemon identity (on the host)

The identity of a daemon on the Odoo host is its service account, which it already authenticates as (an API key). That
needs no new key or transport. For a daemon on another machine, use mutual TLS in front of Odoo and a per-daemon
service account; the certificate then proves which machine, the account says which daemon. This module creates and
stores no key or certificate.

### Lint

`hams_shared/tools/check_private_by_default.py` fails when any `rpc_exposure.json` lists a public method of our models under
`internal`: each must be `@api.private` (or a leading-underscore name) or declared with `@public_rpc`. The list reached zero on
2026-11-12.

## Anchor entries (whitelist)

Every anchor of the whitelist has its entry here and in the story. In brief, the call interface of the ones a caller uses:

### `hams_rpc_guard:public_rpc`

**Purpose.** Decorator declaring a model method callable over RPC. **Call interface.** `public_rpc(func=None, *,
portal=False, clients=())`; bare or with arguments; returns the function itself, unchanged but for one attribute.
`portal` (`bool`) also admits portal and public users; `clients` (tuple of `str` logins) also admits those service
accounts. **Special concerns.** Pure; no I/O; put it directly above the `def` or below `@api.model`. **Test:**
`test_exposure.TestDecorator`.

### `hams_rpc_guard:decide`

**Purpose.** The decision table. **Call interface.** `decide(call, exposure, strict_stock=False)` with a `policy.Call`
and an `exposure.Exposure`; returns `(policy.ALLOW | policy.DENY, reason)`, `reason` one of `portal-listed`,
`strict-portal`, `strict-public`, `client-listed`, `client-not-listed`, `listed`, `managed-not-listed`, `core`,
`stock-listed`, `strict-stock`, or those prefixed `unmapped-client-`. **Errors.** None. **Concurrency.** Pure.
**Test:** `test_policy.TestDecision`.

### `hams_rpc_guard:check_rpc_call`

**Purpose.** Judge one RPC call: read the parameters, build the `Call`, decide, count and refuse. **Call interface.**
`check_rpc_call(model, name)`; `model` a recordset (its `env.user` is the caller), `name` the method name; returns
nothing; raises `AccessError` in enforce mode when denied. **Special concerns.** Reads three parameters through
`zero_sudo.security.utils._get_system_param` (cached) and one cursor query per registry; a parameter read failure is
logged and read as the strict value. **Test:** `test_guard.TestWrapper`.

### `hams_rpc_guard:install` and `hams_rpc_guard:post_load_hook`

**Purpose.** Wrap `odoo.service.model.get_public_method`, `execute_cr` and `odoo.addons.rpc.controllers.json2.get_public_method`
exactly once. **Call interface.** No arguments, no return value; idempotent. **Special concerns.** Runs at module load,
before the first request; swaps module-level names, not Odoo source files. **Test:** `test_guard.TestWrapper`.

### `hams_rpc_guard:load_exposure`, `hams_rpc_guard:scan_main` and the other scanner anchors

`load_exposure(cr, registry)` returns the merged declarations of the installed addons, read once per registry.
`exposure_scan.py` is the generator and checker of the transitional lists (command line above); `scan_source`,
`scan_addon`, `scan_model_of_class`, `scan_render`, `scan_managed_addons`, `scan_orm_names`, `scan_caller_reference` and `scan_daemon_pairs` are
its steps. `scan_daemon_pairs` finds the literal pairs of `execute`/`call`/`_json2_call` calls, of a batch helper
(`push_batches_to_odoo(client, logger, rows, "model", "method")`) and of a caller passed along (`asyncio.to_thread(client.execute, "model", "method")`). All are pure functions of source text. Tests: `test_exposure.TestScanner`, `test_repository`.

### `hams_rpc_guard:certificate_identity`

**Purpose.** The mutual-TLS identity of the request. **Call interface.** `certificate_identity(utils)`; `utils` a
`zero_sudo.security.utils` recordset; returns `"cert:<subject>"` or `""`. **Special concerns.** Reads two parameters;
believes the header only from a loopback peer inside `trusted_proxies`; never widens (see `policy.decide`). **Test:**
`test_guard.TestWrapper`.

### `hams_rpc_guard:peer_is_trusted`

**Purpose.** Whether the socket peer is a loopback address inside the trusted-proxy networks. **Call interface.**
`peer_is_trusted(environ, trusted_proxies)`; returns `bool`; pure. Stricter than `resolve_client`: the peer must also
be a loopback address. **Test:** `test_endpoint_gate.TestResolveClient`, `test_guard.TestWrapper` (certificate identity).
