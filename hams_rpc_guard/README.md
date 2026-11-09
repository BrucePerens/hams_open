# HAMS RPC Guard (`hams_rpc_guard`)

*Copyright © Bruce Perens K6BP.*
SPDX-License-Identifier: AGPL-3.0-or-later

Odoo's generic RPC endpoints let anyone who holds a login, a password or an API key call any public method of any
model. This module makes them answer only the machine Odoo runs on, unless the operator opens them. It works on a
stock Odoo 19 Community server (it needs only core Odoo, `rpc`, `zero_sudo` and `knowledge`), and its defaults are the
safe ones with no configuration.

## Contents

1. [What it does](#what-it-does)
2. [Settings](#settings)
3. [How "on this machine" is decided](#how-on-this-machine-is-decided)
4. [Operators: nginx or Cloudflare instead, or as well](#operators-nginx-or-cloudflare-instead-or-as-well)
5. [Transport and identity: unix socket, TLS and mutual TLS](#transport-and-identity)
6. [Anchor entries](#anchor-entries)

## What it does

| Endpoint | Family | Default | Why |
|---|---|---|---|
| `/xmlrpc/<service>`, `/xmlrpc/2/<service>`, `/jsonrpc` | `legacy-rpc` | `loopback` | Deprecated in Odoo 19, removal scheduled for 22. The body carries a login and password, so each is a brute-force target. Nothing of ours calls them from off the box. |
| `/json/2/<model>/<method>` | `json2` | `any` | The supported API for a daemon on another machine (bearer API key). Production's FCC sync runs off the Odoo host, so closing it by default would break working installs. An operator whose daemons are all on the host sets it to `loopback`. |

A refused request receives a plain 404, as if the route did not exist. The refusal runs before Odoo checks the
credentials, so a refused JSON-2 request never has its bearer key looked at. `/web/dataset/call_kw` and
`/web/dataset/call_button` (the browser's own calls) are **not** touched by the endpoint guard; browsers cannot be
limited to localhost.

The public (anonymous) visitor cannot use `call_kw` or `call_button` at all: Odoo registers both routes `auth="user"`
and answers the public user with "session expired". `tests/test_endpoint_gate.py` pins that, so a future route or
`auth` override that lets the public user through fails a test.

## Settings

System parameters (Settings, Technical, System Parameters). None is created by the module; unset means the default.

| Parameter | Values | Default | Meaning |
|---|---|---|---|
| `hams_rpc_guard.legacy_access` | `loopback`, `any`, `none` | `loopback` | Who may use `/xmlrpc...` and `/jsonrpc`. |
| `hams_rpc_guard.json2_access` | `loopback`, `any`, `none` | `any` | Who may use `/json/2/...`. |
| `hams_rpc_guard.trusted_proxies` | comma-separated loopback addresses or networks | empty | A reverse proxy on this machine that may forward a request whose true client is also on this machine. |

`loopback` admits only a request whose true client is on this machine, `any` is the stock Odoo behaviour, `none`
refuses everything. A value that is not one of the three is read as `loopback` (fail closed), and so is a
parameter that cannot be read. Refusals are counted per worker and logged at the first and at every power of ten
(family and count only: never an address, login, model, method, header or body).

## How "on this machine" is decided

Behind nginx or Cloudflare Tunnel the TCP peer of every request is `127.0.0.1`, so the peer alone proves nothing.
A request is local when both hold:

1. the socket peer is a loopback address (the peer is read from werkzeug's ProxyFix record `werkzeug.proxy_fix.orig`
   when `proxy_mode` has rewritten `REMOTE_ADDR`), or there is no peer address (a Unix socket); and
2. it carries none of `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto`, `X-Forwarded-Server`, `X-Real-IP`,
   `Forwarded`, `Via`, `CF-Connecting-IP`, `CF-Ray`, `True-Client-IP`.

If forwarding headers are present the request is refused, except when the peer is in `hams_rpc_guard.trusted_proxies`
and every client address the proxy supplied (`X-Forwarded-For`, `Forwarded`, `CF-Connecting-IP`, `X-Real-IP`,
`True-Client-IP`) is itself a loopback address and at least one was supplied. A visitor cannot take a header away
that nginx or Cloudflare adds; a visitor can forge `X-Forwarded-For: 127.0.0.1`, which is why a forwarding header is
a refusal and the trusted-proxy rule requires the *proxy* (not the header) to be trusted.

Limitation: a reverse proxy configured to add no forwarding header at all cannot be told apart from a daemon.
Odoo's deployment guide requires `proxy_mode` and the `X-Forwarded-*` headers; do that.

## Operators: nginx or Cloudflare instead, or as well

The module is the default so that every installer is protected. An operator can add the same refusal at the edge:

```nginx
location ~ ^/(xmlrpc/|jsonrpc$) { return 404; }
```

On Cloudflare, a WAF custom rule `(starts_with(http.request.uri.path, "/xmlrpc/")) or (http.request.uri.path eq "/jsonrpc")`
with action Block (the `cloudflare` module's default WAF rules already block `/xmlrpc`). Both are stopgaps in the
sense of the project's security rule: they protect only the hosts that have them; the module is what protects everyone.

## Transport and identity

**Unix socket.** Odoo 19 cannot listen on a Unix socket: its HTTP server creates `AF_INET`/`AF_INET6` sockets
(`odoo/service/server.py`, threaded and prefork servers, and the systemd socket-activation path). A `SO_PEERCRED`
identity (the connecting process's uid and gid, read from the kernel) therefore cannot be had from Odoo directly. The
supported on-box route is a daemon talking to `127.0.0.1:8069` with no forwarding headers, which this module admits.

**TLS on loopback.** Encrypting loopback traffic adds little against a local attacker: a process that can read
the daemon's memory or the loopback interface already has the daemon's API key. It adds something only if each daemon
has its own client key, because the certificate then gives a per-daemon identity (below), not because of the
encryption.

**Mutual TLS, off-box clients only.** For a daemon on another machine (production's FCC sync is one), terminate TLS at
nginx with `ssl_verify_client on;` and a CA the operator controls, one client certificate per daemon, and map the
verified subject to the per-client allowlist (a later change in this module series). This module neither creates nor
stores any key or certificate: key material is provisioning, and nothing here touches a backup key.

**Recommendation.** On the Odoo host: a daemon calls `127.0.0.1` with its own service-account API key (the account is
its identity; the per-client allowlist in the next change keys on it). Off the host: mutual TLS in front of Odoo, and
`hams_rpc_guard.json2_access = any` only for those. Do not run daemons as root or as the `odoo` user.

## Anchor entries

### `hams_rpc_guard:endpoint_family`

**Purpose.** Classifies a request path as one of the two guarded endpoint families, or neither. Called for every
request by `ir.http._authenticate` (below).

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | The request path without the query string. Anything that is not a `str` is "neither". |

**Returns.** `"legacy-rpc"` for exactly `/jsonrpc` and anything under `/xmlrpc/`; `"json2"` for `/json/2` and anything
under `/json/2/`; `None` otherwise. `/xmlrpc.php` (the WordPress probe scanners send), `/jsonrpc/`, `/json/21`,
`/json/version`, `/web/version` and the web client's `/web/dataset/...` are `None`. **Errors.** None. **Concurrency.**
Pure. **Test:** `test_endpoint_gate.TestEndpointFamily`.

### `hams_rpc_guard:origin_is_local`

**Purpose.** Decides whether the true client of a request is on this machine (rules above).

| Argument | Type | Meaning |
|---|---|---|
| `environ` | `dict` | The WSGI environment (`REMOTE_ADDR`, `werkzeug.proxy_fix.orig`). |
| `headers` | `Headers` | The request headers; lookups are case-insensitive. |
| `trusted_proxies` | list of `ip_network` | From `parse_trusted_proxies`; default `()`. |

**Returns.** `True` only when the rules hold; any unparseable address is `False`. **Errors.** None. **Concurrency.**
Pure. **Security.** Never trusts a header's value for admission unless the peer itself is trusted. **Test:**
`test_endpoint_gate.TestOriginIsLocal`.

### `hams_rpc_guard:policy_allows`

**Purpose.** The policy table: `any` admits everyone, `none` admits no one, `loopback` (and anything else) admits a
local request. Arguments: `policy` (`str`), `local` (`bool`). Returns `bool`. Pure. **Test:**
`test_endpoint_gate.TestPolicy`.

### `hams_rpc_guard:endpoint_gate`

**Purpose.** `ir.http._authenticate`, extended: before the core authenticates, a guarded family whose policy refuses
this request raises werkzeug `NotFound` (HTTP 404). Reads the three parameters through
`zero_sudo.security.utils._get_system_param` (whitelisted there); a read failure is logged and read as `loopback`.
Side effects: a per-worker counter and a log line at powers of ten. No database write. **Test:**
`test_endpoint_gate.TestRpcEndpointGate`, `TestPublicVisitorCannotUseTheWebClientRpc`.

### `hams_rpc_guard:post_init_hook`

**Purpose.** Installs the module's documentation article (`data/documentation.html`) through the knowledge module.
Takes the environment; returns nothing; sets no system parameter, so an operator's choice is never overwritten and
the safe defaults are the ones in code. **Test:** `test_endpoint_gate.TestPostInitHook`.
