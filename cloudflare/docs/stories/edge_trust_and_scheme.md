# Story: Trusting the Cloudflare Edge Correctly

As a **Developer/Administrator**,
I want Odoo to know a request really is HTTPS, and to trust Cloudflare's own edge headers only
from a peer that is genuinely Cloudflare, so that session cookies, redirects, and IP-based trust
decisions are correct regardless of which real deployment topology (Tunnel, or plain reverse-proxy)
this installation uses.

## Scenario: HTTPS detection behind a Cloudflare Tunnel

1. `cloudflared` forwards to the origin over plain loopback HTTP and sends `CF-Visitor:
   {"scheme":"https"}` rather than `X-Forwarded-Proto`, so Odoo's own `request.httprequest.scheme`
   would otherwise never report "https" at all behind this deployment's real edge -- wrong for
   session-cookie `Secure` flags, absolute-URL generation, and redirects alike
   `[@ANCHOR: wsgi_proxy_scheme_fix]`.
2. The fix patches `odoo.http.Application.__call__`, the actual WSGI entry point, so it can set the
   raw `environ["wsgi.url_scheme"]` before Werkzeug's `Request.scheme` is ever read -- but only when
   the request's peer is trusted (loopback, the Tunnel case; or a Cloudflare-published/admin-custom
   IP range, the non-Tunnel "orange-cloud" case), so an untrusted peer can never forge a plain-HTTP
   connection into looking like HTTPS `[@ANCHOR: wsgi_proxy_scheme_fix_call]`.
3. That peer-trust check itself -- is this REMOTE_ADDR loopback, or inside the effective trusted-IP
   range list -- is shared, env-less logic the WSGI hook can call before any Odoo database cursor
   exists for the request `[@ANCHOR: is_trusted_cf_peer]`.

## Scenario: Trusting Cloudflare's published IP ranges for "orange-cloud" (non-Tunnel) deployments

4. A self-hosted admin running Cloudflare in front of Odoo WITHOUT a Tunnel has a real network peer
   (not loopback) on every request, so the loopback-only check above can never match it. This
   module merges an auto-fetched Cloudflare-published range list with an admin-supplied custom list
   so that case can trust CF-* headers too `[@ANCHOR: trusted_ip_ranges]`. The whole feature is
   off by default behind one explicit "Trust Cloudflare's Published IP Ranges" setting: a
   Tunnel-only deployment's only peer is loopback, so until the admin turns it on the effective
   list is empty, auto-fetched snapshot and custom additions alike.
5. The auto-fetched half refreshes daily from `https://www.cloudflare.com/ips-v4`/`ips-v6`, seeded
   from a baked-in snapshot so the feature works before the cron's first run, and never overwrites
   the last known-good list on a failed/empty/malformed fetch
   `[@ANCHOR: cron_refresh_cloudflare_ip_ranges]`.
6. The admin's own custom additions live in a separate settings field, never touched by that cron,
   so a manual edit is never silently clobbered by the next scheduled refresh
   `[@ANCHOR: COMM_trusted_ip_ranges_settings_fields]`; a "Refresh now" button on the same settings
   page runs the identical cron logic on demand and reloads the page so the admin sees the new list
   immediately `[@ANCHOR: COMM_action_refresh_cloudflare_trusted_ip_ranges]`.
7. The merged, de-duplicated auto+custom list is what both the `env`-bearing peer-trust check
   `[@ANCHOR: is_trusted_cf_peer_with_env]` and the env-less WSGI hook actually read
   `[@ANCHOR: get_effective_trusted_ip_ranges]`; since the WSGI hook runs before any database
   connection exists, the merged list is also published to Redis on every cron refresh and every
   settings save, so it's never more than one tick stale `[@ANCHOR: publish_trusted_ip_ranges_to_redis]`.
   A module upgrade re-publishes it too, so an empty list replaces any wider one an older version
   left in Redis `[@ANCHOR: COMM_republish_trusted_ip_ranges_on_module_load]`.

## Scenario: Rotating Cloudflare credentials invalidates the cross-worker cache

8. `website`'s Cloudflare credential fields (API token, zone/account id, Turnstile secret) feed a
   `@distributed_cache()`'d method with a 24h TTL, so a plain `website.write()` that changes one of
   them must explicitly bust that Redis-backed cache -- otherwise every OTHER Odoo worker (and a
   fresh `odoo shell`) would keep using the stale credential for up to a day
   `[@ANCHOR: COMM_website_write_busts_credential_cache]`.

**Status:** Verified by `[@ANCHOR: test_wsgi_proxy_scheme_fix_sets_https_for_trusted_cf_visitor]`,
`[@ANCHOR: test_trusted_ip_ranges]`, and `[@ANCHOR: COMM_test_credential_write_busts_distributed_cache]`.
