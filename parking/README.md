# Parking (`parking`)

*Copyright (c) HAMS project. Licensed under AGPL-3.0-or-later.*

One small Odoo instance answers for any number of parked domains. Install it **only** in a dedicated
instance that sits behind a Cloudflare Tunnel catch-all rule (see `docs/proposals/MULTI_TENANT_ODOO.md` in
hams_com and ADR 0105): it takes over every public request of that instance.

## What a visitor gets

* A parked page, a 301/302 redirect to a fixed URL, a for-sale page with a contact form, or `410 Gone`,
  chosen per domain in **Parking > Domains**.
* A few kilobytes of self-contained HTML: no script, no cookie, a content security policy that allows
  none, `robots.txt` and `X-Robots-Tag` asking search engines not to index (per domain switch).
* A plain, non-cacheable 404 for a host that is not in the table (`parking.unknown_host_policy` =
  `not_found`, or `default_page`).
* No route, controller or backend page besides these: the Odoo login, backend, JSON-RPC, XML-RPC and
  websocket routes are not routes on a public host. (Odoo's own `/<module>/static/*` files are still
  served before routing, so a parked name does reveal that it is Odoo.)

## Administration

The backend answers only when the socket peer is a loopback address and the request carries no
`CF-Ray`/`CF-Connecting-IP` header: use `ssh -L <port>:127.0.0.1:<port> server` and browse
`http://localhost:<port>`. Bulk changes: `hams_shared/tools/parking_ctl.py`.

## Security notes

* The host is read from the original `Host` header, never from `X-Forwarded-Host` (Odoo's proxy mode
  would otherwise let a visitor of one domain make the instance answer as another, and Cloudflare would
  cache the answer under the first name).
* A redirect target is a fixed absolute http(s) URL; with "keep the path" only the path and query are
  appended, so the target host cannot change.
* The for-sale form carries a stateless token bound to the host and the render time; bots get the same
  answer as people and nothing is stored; one address may send five inquiries an hour.
* The public handler runs as one service account that can read domains and write inquiries and nothing
  else (no `zero_sudo`: a tenant instance has no Redis). The module depends on `web` and `mail`; `mail`
  brings cron jobs, which is why a tenant keeps `max_cron_threads = 1`.

## Tests

`parking/tests` (run with the project's Hams test base classes, which need `zero_sudo` importable).
