# Parking (`parking`)

*Copyright (c) HAMS project. Licensed under AGPL-3.0-or-later.*

Answers for any number of parked domains from one table, inside the Odoo that also serves the main
site and its tenant sites (hams_shared ADR 0106). It is a small plug-in of `tenant_sites`, which decides
which hostnames are parked: a request that arrives through Cloudflare for a hostname in `parking.domain`
(and only for such a hostname) gets the page below; every other hostname is the main site's, a tenant
website's, or an uncached 404, and never reaches this module's pages.

## What a visitor gets

* A parked page, a 301/302 redirect to a fixed URL, a for-sale page with a contact form, or `410 Gone`,
  chosen per domain in **Parking > Domains**.
* A few kilobytes of self-contained HTML: no script, no cookie, a content security policy that allows
  none, `robots.txt` and `X-Robots-Tag` asking search engines not to index (per domain switch).
* On a parked domain no route, controller or backend page besides these exists: the Odoo login,
  backend, JSON-RPC, XML-RPC and websocket routes are not routes there, and the main site's pages are
  never shown. (Odoo's own `/<module>/static/*` files are still served before routing.)
* An unknown hostname (neither main, tenant nor parked) gets a plain, non-cacheable 404, or the default
  parked page when `parking.unknown_host_policy` is `default_page`.

## Administration

Requests whose Host is not a domain name (`localhost`, an address: this machine's daemons, an operator on
loopback) are never classified, so the backend works for them as usual. Bulk changes: `hams_shared/tools/parking_ctl.py`
(JSON-2 API) or the setup script `devbox_tools/tenants_in_odoo_setup.py` in hams_com.

## Security notes

* The host is read from the original `Host` header, never from `X-Forwarded-Host` (Odoo's proxy mode
  would otherwise let a visitor of one domain make Odoo answer as another, and Cloudflare would
  cache the answer under the first name).
* A redirect target is a fixed absolute http(s) URL; with "keep the path" only the path and query are
  appended, so the target host cannot change.
* The for-sale form carries a stateless token bound to the host and the render time; bots get the same
  answer as people and nothing is stored; one address may send five inquiries an hour.
* The public handler runs as one service account that can read domains and write inquiries and nothing
  else.
* A parked domain may not match a main-site pattern or a tenant hostname (`parking.domain` refuses it).

## Tests

`parking/tests` (run with the project's Hams test base classes).
