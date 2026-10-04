# tenant_sites

Serve other sites (perens.com, postopen.org, ...) as websites of this one Odoo, isolated from the
main site. The architecture decision is `hams_shared/docs/adrs/0106_tenants_inside_one_odoo.md`.

## How a request is classified (`models/ir_http.py`)

Classification depends on the hostname the client sent (`werkzeug.proxy_fix.orig`, never an
`X-Forwarded-Host`), and the request is refused (400) when it differs from the one Odoo would use. A
request whose Host is not a domain name (`localhost`, an IP address, a one-word name such as the
`odoo` host this machine's own daemons may use) is never classified and is served as before, which
keeps the JSON-2 callers, the test harness and operators on loopback working; through the tunnel
(it carries `CF-Ray`, which Cloudflare's edge sets itself) such a Host is refused. Nothing depends on
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

## Not done here

DNS records and Cloudflare Custom Hostnames. The `cloudflare` module has a `cloudflare.dns.record` model but it is data only (no push), and
`edge.routing.domain` provisions a real Custom Hostname on create, so do not create one for a zone that
is already in the account.
