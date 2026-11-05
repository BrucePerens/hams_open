# tenant_sites

Serve other sites (perens.com, postopen.org, ...) as websites of this one Odoo, isolated from the
main site. The architecture decision is `hams_shared/docs/adrs/0106_tenants_inside_one_odoo.md`.

## How a request is classified (`models/ir_http.py`)

Classification depends on the hostname the client sent (`werkzeug.proxy_fix.orig`, never an
`X-Forwarded-Host`), and the request is refused (400) when it differs from the one Odoo would use. A
request whose Host is not a domain name (`localhost`, an IP address, a one-word name such as the
`odoo` host this machine's own daemons may use) is never classified and is served as before, which
keeps the JSON-2 callers, the test harness and operators on loopback working. Nothing depends on
`CF-Ray` being present for a real domain name, so an operator can check a tenant over loopback with a
`Host:` header.

| Kind | Hostname | Served |
|---|---|---|
| main | matches `tenant.site.own.host` (or an `edge.routing.domain`) | exactly as before |
| tenant | in `tenant.site.host` | that website only, GET/HEAD only, public paths only |
| parking | a `parking.domain` (module `parking`) | the parked page |
| unknown | anything else (once main hostnames are configured) | uncached 404 |

A tenant hostname selects the tenant's website (`models/website.py`) and, while the request is
served, `website.page`, `website.menu`, `website.rewrite`, `blog.blog` and `blog.post` only find
that website's own records (`models/scoped_content.py`), so records shared by every website (empty
`website_id`) never show on a tenant. Routes: only the paths of `utils.TENANT_PATH_ALLOW`, only
from `website`, `website_blog`, `web`, `http_routing`, `edge_cache` controllers (`utils.tenant_module_allowed`);
every other path falls through to the tenant website's own page/redirect lookup, then 404. The
backend, login, `/jsonrpc`, `/xmlrpc`, `/json/2`, `/websocket`, `/website/info` are therefore 404 on a tenant host, and so is
any route of a `hams_com` module.

## Tunnel guard (`models/cloudflare_tunnel.py`)

`cloudflare.tunnel._ingress_problems` is extended: a push is refused while any tenant hostname exists
and a rule has a path but no hostname (it would send tenant and parked hostnames to a daemon port).
`cloudflare.tunnel._build_ingress()` returns the list a push would send, with no network call.

It also refuses a push that drops a tenant's explicit `/static/` rule (`_static_rule_problems`, `[@ANCHOR: tenant_sites:COMM_static_rule_problems]`). A tenant site whose files are served
by a separate static file server records it in `tenant.site.static_service` (for example
`http://localhost:18201` for perens.com); every hostname of such a site must then have a rule with that
hostname, the path `^/static/` and that service, or the push is refused with the missing hostnames named
(the catch-all would otherwise send them to Odoo, which answers 404). The 1.1 migration adopts the sites that
already have those rules, so the guard protects them from the first push.

## REQUEST_URI (`models/ir_http.py`)

### `[@ANCHOR: tenant_sites:ensure_request_uri]` `ir.http._tenant_ensure_request_uri()`

**Purpose.** Odoo's `website` module reads `request.httprequest.environ['REQUEST_URI']` without a default in `_serve_redirect` and in the canonical
URL of every page it renders. Werkzeug's server (Odoo's HTTP workers) sets that key; the gevent server of the websocket worker (port 8072) does not.
nginx sends everything under the prefix `/websocket` to that worker, so `/websocket.js` and `/websocket.js.map` (browsers and extensions ask for them) reached
Odoo without the key and ended in `KeyError` and HTTP 500 (tickets 134 and 136, 2026-11-03). Called first thing by `_serve_fallback`, which every
request that matched no route passes through.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| (none) | | A classmethod; works on the current `odoo.http.request`. |
| returns | `None` | Nothing. |

Effect: when `REQUEST_URI` is missing it is set to the request's `RAW_URI` if the server provided one, otherwise to `PATH_INFO` plus `?` and `QUERY_STRING`
when there is a query string. A value that is already present is never changed. Errors: none; it needs an active `request`.

**Special concerns.** Synchronous, no database, no locks, idempotent. It only adds a key to the request's own WSGI environment, so it cannot change which
page is served; it is not a security check and the value is used only to look up redirects and to build canonical URLs.

**See also.** Test `test_a_request_without_request_uri_is_a_404_not_a_server_error` in `tests/test_http.py` (a real request with the key removed from the server's environment);
`[@ANCHOR: tenant_sites:COMM_serve_fallback]`, the caller.

## Not done here

DNS records and Cloudflare Custom Hostnames. The `cloudflare` module has a `cloudflare.dns.record` model but it is data only (no push), and
`edge.routing.domain` provisions a real Custom Hostname on create, so do not create one for a zone that
is already in the account.
